use std::ops::Range;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

#[cfg(feature = "png")]
use async_tiff::decoder::DecoderRegistry;
use async_tiff::metadata::TiffMetadataReader;
use async_tiff::metadata::cache::ReadaheadMetadataCache;
use async_tiff::tags::{Compression, PhotometricInterpretation, PlanarConfiguration, SampleFormat};
use async_tiff::{ImageFileDirectory, TileByteRange};
use bytes::Bytes;
use futures::FutureExt as _;

use crate::backend::{AsyncBackend, TiffBridge};
use crate::geo::{Georef, aligned_tile, match_zoom};
use crate::tile::{TileCoord, mercator_to_lnglat};
use crate::{CogError, CogResult};

/// How many bytes to read up front when opening a COG. GDAL puts all headers at the start,
/// so one request usually covers every IFD of a typical file.
pub const DEFAULT_PREFETCH: u64 = 32 * 1024;

/// The format of the tiles a COG produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TileFormat {
    /// JPEG, passed through from the file.
    Jpeg,
    /// WebP, passed through from the file.
    Webp,
    /// PNG, re-encoded from losslessly compressed (or JPEG RGBA) 8-bit tiles.
    Png,
}

impl TileFormat {
    /// The MIME type of the tile data.
    #[must_use]
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Png => "image/png",
        }
    }

    /// The usual file extension, without a dot.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Png => "png",
        }
    }
}

/// The tiles a COG provides: their zoom range, size, format and extent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CogInfo {
    /// The lowest zoom with tiles (the coarsest overview).
    pub min_zoom: u8,
    /// The highest zoom with tiles (the full-resolution image).
    pub max_zoom: u8,
    /// Tile width and height in pixels.
    pub tile_size: u32,
    /// The format of every tile.
    pub format: TileFormat,
    /// The image extent in `EPSG:3857` meters: `[min_x, min_y, max_x, max_y]`.
    pub mercator_bounds: [f64; 4],
    /// The image extent in degrees: `[west, south, east, north]`.
    pub bounds: [f64; 4],
}

/// How stored tiles are turned into returned tiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    /// Return the stored bytes, inserting `JPEGTables` for JPEG.
    Passthrough,
    /// Decode the stored tile and re-encode it as PNG.
    #[cfg(feature = "png")]
    Png,
}

/// One image (full resolution or overview) of the COG, providing the tiles of one zoom level.
#[derive(Debug)]
struct Level {
    zoom: u8,
    /// Tile columns (at `zoom`) the image covers.
    cols: Range<u32>,
    /// Tile rows (at `zoom`) the image covers.
    rows: Range<u32>,
    ifd: ImageFileDirectory,
}

impl Level {
    /// The internal tile column and row holding `coord`, if the image covers it.
    fn position(&self, coord: TileCoord) -> Option<(usize, usize)> {
        if !self.cols.contains(&coord.x()) || !self.rows.contains(&coord.y()) {
            return None;
        }
        let col = coord.x() - self.cols.start;
        let row = coord.y() - self.rows.start;
        Some((col as usize, row as usize))
    }
}

/// Reads `WebMercatorQuad` tiles from one Cloud Optimized `GeoTIFF`.
///
/// The COG must be in `EPSG:3857`, and every image in it (the full-resolution image and each
/// overview) must be tiled on the `WebMercatorQuad` grid: its resolution matches a zoom level and
/// its internal tiles line up with that zoom's tiles. This is what GDAL produces with
/// `-of COG -co TILING_SCHEME=GoogleMapsCompatible`. Each returned tile is one internal tile,
/// so no resampling happens: JPEG and WebP tiles are returned as stored, others are re-encoded as PNG.
///
/// ```no_run
/// # #[cfg(feature = "fs")]
/// # async fn run() -> cog_tiles::CogResult<()> {
/// use cog_tiles::{AsyncCogReader, FileBackend, TileCoord};
///
/// let reader = AsyncCogReader::try_from_backend(FileBackend::open("ortho.tif").await?).await?;
/// let tile = reader.get_tile(TileCoord::new(16, 32748, 21794)?).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct AsyncCogReader<B> {
    bridge: TiffBridge<B>,
    info: CogInfo,
    /// Levels indexed by `zoom - info.min_zoom`.
    levels: Vec<Option<Level>>,
    /// Why images that cannot be read as tiles were left out.
    skipped: Vec<String>,
    encoding: Encoding,
    #[cfg(feature = "png")]
    decoders: Arc<DecoderRegistry>,
}

impl<B: AsyncBackend> AsyncCogReader<B> {
    /// Opens a COG, reading its headers and validating that it can be read as tiles.
    ///
    /// # Errors
    /// Returns an error if the headers cannot be read, or [`CogError::Unsupported`] if the COG
    /// is not in Web Mercator, not aligned to the tile grid, or uses an unsupported pixel layout.
    pub async fn try_from_backend(backend: B) -> CogResult<Self> {
        Self::try_from_backend_with_prefetch(backend, DEFAULT_PREFETCH).await
    }

    /// Like [`try_from_backend`](Self::try_from_backend), reading `prefetch` bytes in the first request.
    ///
    /// # Errors
    /// See [`try_from_backend`](Self::try_from_backend).
    pub async fn try_from_backend_with_prefetch(backend: B, prefetch: u64) -> CogResult<Self> {
        let bridge = TiffBridge(Arc::new(backend));
        let ifds = read_ifds(&bridge, prefetch).await?;
        let location = bridge.0.location();

        let base = ifds
            .first()
            .ok_or_else(|| CogError::unsupported(location, "it contains no images"))?;
        let georef = Georef::from_ifd(base, location)?;
        let tile_size = tile_size(base, location)?;
        let (format, encoding) = choose_encoding(base, location)?;
        let width = f64::from(base.image_width()) * georef.resolution;
        let height = f64::from(base.image_height()) * georef.resolution;
        for (index, ifd) in ifds.iter().enumerate() {
            if is_image(index, ifd) {
                check_same_layout(base, ifd, index, tile_size, location)?;
            }
        }

        // Images that do not match a zoom level or are off the tile grid are left out, so a file
        // whose overviews are misaligned still provides the levels that are aligned.
        let mut levels: Vec<Level> = Vec::new();
        let mut skipped = Vec::new();
        for (index, ifd) in ifds.into_iter().enumerate() {
            if !is_image(index, &ifd) {
                continue;
            }
            match make_level(ifd, index, &georef, width, tile_size, location)? {
                Ok(level) if levels.iter().any(|l| l.zoom == level.zoom) => skipped.push(format!(
                    "image {index} has the resolution of zoom {}, like an earlier image",
                    level.zoom
                )),
                Ok(level) => levels.push(level),
                Err(reason) => skipped.push(reason),
            }
        }
        let Some(max_zoom) = levels.iter().map(|l| l.zoom).max() else {
            return Err(CogError::unsupported(
                location,
                if skipped.is_empty() {
                    "it contains no images".to_owned()
                } else {
                    skipped.join("; ")
                },
            ));
        };
        let mercator_bounds = [
            georef.origin_x,
            georef.origin_y - height,
            georef.origin_x + width,
            georef.origin_y,
        ];
        let [west, south] = mercator_to_lnglat(mercator_bounds[0], mercator_bounds[1]);
        let [east, north] = mercator_to_lnglat(mercator_bounds[2], mercator_bounds[3]);

        let min_zoom = levels.iter().map(|l| l.zoom).min().unwrap_or(max_zoom);
        let mut by_zoom: Vec<Option<Level>> = (min_zoom..=max_zoom).map(|_| None).collect();
        for level in levels {
            let slot = usize::from(level.zoom - min_zoom);
            by_zoom[slot] = Some(level);
        }

        Ok(Self {
            bridge,
            info: CogInfo {
                min_zoom,
                max_zoom,
                tile_size,
                format,
                mercator_bounds,
                bounds: [west, south, east, north],
            },
            levels: by_zoom,
            skipped,
            encoding,
            #[cfg(feature = "png")]
            decoders: Arc::new(DecoderRegistry::default()),
        })
    }

    /// What this COG offers as a tile source.
    #[must_use]
    pub fn info(&self) -> &CogInfo {
        &self.info
    }

    /// Why images that cannot be read as tiles were left out, e.g. an overview that is not aligned to
    /// the tile grid. Empty when every image is used.
    #[must_use]
    pub fn skipped_images(&self) -> &[String] {
        &self.skipped
    }

    /// The backend this reader reads from.
    #[must_use]
    pub fn backend(&self) -> &B {
        &self.bridge.0
    }

    /// Whether the image at zoom `coord.z()` has an internal tile for `coord`.
    /// Returns `false` for zooms the COG has no image for.
    #[must_use]
    pub fn contains(&self, coord: TileCoord) -> bool {
        self.level(coord.z())
            .is_some_and(|level| level.position(coord).is_some())
    }

    /// Reads one tile, encoded as [`CogInfo::format`].
    ///
    /// Returns `Ok(None)` if the COG has no image for the zoom, the tile is outside the image,
    /// or the tile is sparse (not stored). Re-encoding to PNG happens on the calling task.
    ///
    /// # Errors
    /// Returns an error if the backend fails or the tile cannot be decoded.
    pub async fn get_tile(&self, coord: TileCoord) -> CogResult<Option<Bytes>> {
        let Some(level) = self.level(coord.z()) else {
            return Ok(None);
        };
        let Some((col, row)) = level.position(coord) else {
            return Ok(None);
        };
        let location = self.bridge.0.location();
        let range = match level.ifd.tile_byte_range(col, row) {
            Some(TileByteRange::Chunky(range)) => range,
            Some(TileByteRange::Planar(ranges)) if ranges.len() == 1 => ranges[0].clone(),
            _ => {
                return Err(CogError::unsupported(
                    location,
                    format!("tile {col},{row} of zoom {} has no byte range", level.zoom),
                ));
            }
        };
        if range.is_empty() {
            return Ok(None);
        }
        match self.encoding {
            Encoding::Passthrough => {
                let bytes = self.bridge.0.read(range).await?;
                Ok(Some(match level.ifd.jpeg_tables() {
                    Some(tables) if level.ifd.compression() == Compression::ModernJPEG => {
                        crate::jpeg::merge_tables(tables, bytes)
                    }
                    _ => bytes,
                }))
            }
            #[cfg(feature = "png")]
            Encoding::Png => {
                let tile = level
                    .ifd
                    .fetch_tile(col, row, &self.bridge)
                    .await
                    .map_err(|e| unwrap_tiff_error(location, e))?;
                let array = tile
                    .decode(&self.decoders)
                    .map_err(|e| CogError::tiff(location, e))?;
                let shape = array.shape();
                let async_tiff::TypedArray::UInt8(pixels) = array.data() else {
                    return Err(CogError::unsupported(location, "decoded tile is not 8-bit"));
                };
                crate::encode::encode_png(pixels, shape, self.info.tile_size, location).map(Some)
            }
        }
    }

    fn level(&self, zoom: u8) -> Option<&Level> {
        let index = zoom.checked_sub(self.info.min_zoom)?;
        self.levels.get(usize::from(index))?.as_ref()
    }
}

/// Reads all IFDs, converting `async-tiff` panics on malformed input into errors.
async fn read_ifds<B: AsyncBackend>(
    bridge: &TiffBridge<B>,
    prefetch: u64,
) -> CogResult<Vec<ImageFileDirectory>> {
    let location = bridge.0.location();
    let cache = ReadaheadMetadataCache::new(bridge.clone())
        .with_initial_size(prefetch.min(bridge.0.size()).max(16));
    let parsed = AssertUnwindSafe(async {
        let mut reader = TiffMetadataReader::try_open(&cache).await?;
        reader.read_all_ifds(&cache).await
    })
    .catch_unwind()
    .await;
    match parsed {
        Ok(Ok(ifds)) => Ok(ifds),
        Ok(Err(e)) => Err(unwrap_tiff_error(location, e)),
        Err(panic) => {
            let reason = panic
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("unknown panic");
            Err(CogError::unsupported(
                location,
                format!("the TIFF parser failed on malformed metadata: {reason}"),
            ))
        }
    }
}

/// Recovers our own backend error if `async-tiff` wrapped it, otherwise wraps the TIFF error.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "every other `async-tiff` error is wrapped as it is"
)]
fn unwrap_tiff_error(location: &str, error: async_tiff::error::AsyncTiffError) -> CogError {
    match error {
        async_tiff::error::AsyncTiffError::External(inner) => match inner.downcast::<CogError>() {
            Ok(cog) => *cog,
            Err(other) => CogError::Backend {
                location: location.to_owned(),
                source: other,
            },
        },
        other => CogError::tiff(location, other),
    }
}

fn tile_size(ifd: &ImageFileDirectory, location: &str) -> CogResult<u32> {
    match (ifd.tile_width(), ifd.tile_height()) {
        (Some(w), Some(h)) if w == h && w > 0 => Ok(w),
        (Some(w), Some(h)) => Err(CogError::unsupported(
            location,
            format!("its internal tiles are not square ({w}x{h})"),
        )),
        _ => Err(CogError::unsupported(
            location,
            "it is stored in strips, not tiles",
        )),
    }
}

/// Picks the output format for the full-resolution image's pixel layout.
fn choose_encoding(ifd: &ImageFileDirectory, location: &str) -> CogResult<(TileFormat, Encoding)> {
    let samples = ifd.samples_per_pixel();
    let photometric = ifd.photometric_interpretation();
    let eight_bit = ifd.bits_per_sample().iter().all(|&b| b == 8)
        && ifd.sample_format().iter().all(|&f| f == SampleFormat::Uint);
    if !eight_bit {
        return Err(CogError::unsupported(
            location,
            format!(
                "only 8-bit unsigned samples are supported, found {:?} bits of {:?}",
                ifd.bits_per_sample(),
                ifd.sample_format()
            ),
        ));
    }
    if samples > 1 && ifd.planar_configuration() != PlanarConfiguration::Chunky {
        return Err(CogError::unsupported(
            location,
            "bands are stored separately (PlanarConfiguration=2)",
        ));
    }
    #[cfg(feature = "png")]
    let gray = photometric == PhotometricInterpretation::BlackIsZero && matches!(samples, 1 | 2);
    #[cfg(feature = "png")]
    let rgb = photometric == PhotometricInterpretation::RGB && matches!(samples, 3 | 4);

    let compression = ifd.compression();
    match compression {
        Compression::WebP if matches!(samples, 3 | 4) => {
            Ok((TileFormat::Webp, Encoding::Passthrough))
        }
        // JPEG-in-TIFF stored as RGB labels its components `R`, `G`, `B`, which JPEG decoders
        // (libjpeg, libjpeg-turbo) read as RGB, so it passes through like YCbCr and grayscale.
        Compression::ModernJPEG
            if (matches!(
                photometric,
                PhotometricInterpretation::YCbCr | PhotometricInterpretation::RGB
            ) && samples == 3)
                || (photometric == PhotometricInterpretation::BlackIsZero && samples == 1) =>
        {
            Ok((TileFormat::Jpeg, Encoding::Passthrough))
        }
        #[cfg(feature = "png")]
        Compression::ModernJPEG if rgb => Ok((TileFormat::Png, Encoding::Png)),
        #[cfg(feature = "png")]
        Compression::None
        | Compression::Deflate
        | Compression::OldDeflate
        | Compression::LZW
        | Compression::ZSTD
            if gray || rgb =>
        {
            Ok((TileFormat::Png, Encoding::Png))
        }
        _ => Err(CogError::unsupported(
            location,
            format!(
                "{compression:?} compression with {samples} {photometric:?} samples is not supported{}",
                if cfg!(feature = "png") {
                    ""
                } else {
                    " (the `png` feature is disabled)"
                }
            ),
        )),
    }
}

/// The full-resolution image width in meters, once the first level is known.
/// Checks that an overview is stored the same way as the full-resolution image.
/// Whether an IFD is the full-resolution image or an overview, rather than a mask or another page.
fn is_image(index: usize, ifd: &ImageFileDirectory) -> bool {
    let subfile = ifd.new_subfile_type().unwrap_or(0);
    subfile & 0b100 == 0 && (index == 0 || subfile & 0b001 != 0)
}

fn check_same_layout(
    base: &ImageFileDirectory,
    ifd: &ImageFileDirectory,
    index: usize,
    tile_size: u32,
    location: &str,
) -> CogResult<()> {
    if ifd.tile_width() != Some(tile_size) || ifd.tile_height() != Some(tile_size) {
        return Err(CogError::unsupported(
            location,
            format!("image {index} uses a different internal tile size"),
        ));
    }
    if ifd.compression() != base.compression()
        || ifd.photometric_interpretation() != base.photometric_interpretation()
        || ifd.samples_per_pixel() != base.samples_per_pixel()
        || ifd.bits_per_sample() != base.bits_per_sample()
        || ifd.planar_configuration() != base.planar_configuration()
    {
        return Err(CogError::unsupported(
            location,
            format!("image {index} is stored differently from the full-resolution image"),
        ));
    }
    Ok(())
}

/// Maps one image to the zoom level it provides.
///
/// Returns `Ok(Err(reason))` for an image that cannot be used but does not make the file unusable.
fn make_level(
    ifd: ImageFileDirectory,
    index: usize,
    georef: &Georef,
    base_width: f64,
    tile_size: u32,
    location: &str,
) -> CogResult<Result<Level, String>> {
    let resolution = if index == 0 {
        georef.resolution
    } else {
        base_width / f64::from(ifd.image_width())
    };
    let Some(zoom) = match_zoom(resolution, tile_size) else {
        return Ok(Err(format!(
            "image {index} has {resolution} m pixels, which is not a WebMercatorQuad zoom level for {tile_size} px tiles"
        )));
    };
    let Some(first_tile) = aligned_tile(georef.origin_x, georef.origin_y, zoom, tile_size) else {
        return Ok(Err(format!(
            "image {index} is not aligned to the zoom {zoom} tile grid"
        )));
    };
    let (cols, rows) = ifd
        .tile_count()
        .ok_or_else(|| CogError::unsupported(location, format!("image {index} is not tiled")))?;
    let to_u32 = |v: usize| {
        u32::try_from(v).map_err(|_err| {
            CogError::unsupported(location, format!("image {index} has too many tiles"))
        })
    };
    let tile_count = (to_u32(cols)?, to_u32(rows)?);
    let world = 1_u64 << zoom;
    if u64::from(first_tile.0) + u64::from(tile_count.0) > world
        || u64::from(first_tile.1) + u64::from(tile_count.1) > world
    {
        return Ok(Err(format!(
            "image {index} extends beyond the edge of the world at zoom {zoom}"
        )));
    }
    let (first_col, first_row) = first_tile;
    let (col_count, row_count) = tile_count;
    Ok(Ok(Level {
        zoom,
        cols: first_col..first_col + col_count,
        rows: first_row..first_row + row_count,
        ifd,
    }))
}

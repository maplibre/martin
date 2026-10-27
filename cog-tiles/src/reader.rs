use std::ops::Range;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use async_tiff::decoder::DecoderRegistry;
use async_tiff::metadata::TiffMetadataReader;
use async_tiff::tags::{Compression, PhotometricInterpretation, PlanarConfiguration, SampleFormat, Tag};
use async_tiff::{ImageFileDirectory, TileByteRange};
use bytes::Bytes;
use futures::FutureExt as _;

use crate::backend::{AsyncBackend, TiffBridge};
use crate::bands::{BandCache, BandKey, FileId};
use crate::geo::{Georef, aligned_tile, match_zoom};
use crate::metadata::BlockMetadataCache;
use crate::tile::{TileCoord, mercator_to_lnglat};
use crate::{Chunking, CogError, CogResult, SkippedImage, TilePixels, UnsupportedReason};

/// Bytes read in the first request when opening, which covers all headers of a typical GDAL COG.
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

/// Striped images carry no tile size, so they are cut into tiles of this many pixels.
const STRIPED_TILE_SIZE: u32 = 256;

/// How the pixels of an image are chunked in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Chunks {
    /// Square tiles of this many pixels, each read and decoded on its own.
    Tiles(u32),
    /// Uncompressed full-width strips.
    Strips(Strips),
}

impl Chunks {
    /// Reads how an image is chunked.
    pub(crate) fn of(ifd: &ImageFileDirectory, index: usize, location: &str) -> CogResult<Self> {
        let unsupported = |reason| CogError::unsupported(location, reason);
        match (ifd.tile_width(), ifd.tile_height()) {
            (Some(w), Some(h)) if w == h && w > 0 => return Ok(Self::Tiles(w)),
            (Some(width), Some(height)) => {
                return Err(unsupported(UnsupportedReason::NonSquareTiles { index, width, height }));
            }
            _ => {}
        }
        let Some(offsets) = ifd.strip_offsets() else {
            return Err(unsupported(UnsupportedReason::Untiled { index }));
        };
        if ifd.compression() != Compression::None {
            return Err(unsupported(UnsupportedReason::CompressedStrips {
                index,
                compression: ifd.compression(),
            }));
        }
        let height = ifd.image_height();
        let rows_per_strip = ifd.rows_per_strip().unwrap_or(height).min(height);
        if rows_per_strip == 0 {
            return Err(unsupported(UnsupportedReason::EmptyStrips { index }));
        }
        let strips = Strips {
            rows_per_strip,
            row_bytes: u64::from(ifd.image_width()) * u64::from(ifd.samples_per_pixel()),
        };
        let count = height.div_ceil(rows_per_strip) as usize;
        let byte_counts = ifd.strip_byte_counts().unwrap_or_default();
        if offsets.len() != count || byte_counts.len() != count {
            return Err(unsupported(UnsupportedReason::StripCount {
                index,
                rows_per_strip,
                strips: count,
                offsets: offsets.len(),
                byte_counts: byte_counts.len(),
            }));
        }
        let short = (0..count)
            .zip(byte_counts)
            .find(|&(strip, &byte_count)| byte_count < strips.bytes_of(strip, height));
        if let Some((strip, &byte_count)) = short {
            return Err(unsupported(UnsupportedReason::ShortStrip {
                index,
                strip,
                byte_count,
                needed: strips.bytes_of(strip, height),
            }));
        }
        Ok(Self::Strips(strips))
    }

    /// The width and height of the tiles the image is cut into.
    fn tile_size(self) -> u32 {
        match self {
            Self::Tiles(size) => size,
            Self::Strips(_) => STRIPED_TILE_SIZE,
        }
    }
}

/// Row layout of an uncompressed striped image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Strips {
    /// Rows per strip (the last strip may have fewer).
    rows_per_strip: u32,
    /// Bytes per image row.
    row_bytes: u64,
}

impl Strips {
    /// Expected byte count of `strip` in an image of `height` rows.
    fn bytes_of(self, strip: usize, height: u32) -> u64 {
        let first = strip as u64 * u64::from(self.rows_per_strip);
        (u64::from(height) - first).min(u64::from(self.rows_per_strip)) * self.row_bytes
    }

    /// Byte ranges of `rows`, with adjacent ranges merged.
    fn ranges(self, offsets: &[u64], rows: Range<u64>) -> Vec<Range<u64>> {
        let rows_per_strip = u64::from(self.rows_per_strip);
        let mut ranges: Vec<Range<u64>> = Vec::new();
        let mut y = rows.start;
        while y < rows.end {
            let strip = usize::try_from(y / rows_per_strip).expect("strip indexes fit in usize");
            let next = ((y / rows_per_strip + 1) * rows_per_strip).min(rows.end);
            let start = offsets[strip] + (y % rows_per_strip) * self.row_bytes;
            let end = start + (next - y) * self.row_bytes;
            match ranges.last_mut() {
                Some(last) if last.end == start => last.end = end,
                _ => ranges.push(start..end),
            }
            y = next;
        }
        ranges
    }
}

/// One image (full resolution or overview) of the COG, providing the tiles of one zoom level.
#[derive(Debug)]
struct Level {
    /// Zoom this image serves.
    zoom: u8,
    /// Tile columns (at `zoom`) the image covers.
    cols: Range<u32>,
    /// Tile rows (at `zoom`) the image covers.
    rows: Range<u32>,
    /// Tile or strip layout.
    chunks: Chunks,
    /// Image metadata.
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

/// Where one stored tile is: its image, internal column and row, and bytes.
struct StoredTile<'a> {
    /// Image containing the tile.
    level: &'a Level,
    /// Internal tile column.
    col: usize,
    /// Internal tile row.
    row: usize,
    /// Byte range in the file.
    range: Range<u64>,
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
    /// Backend wrapped for `async-tiff`.
    bridge: TiffBridge<B>,
    /// Tile source metadata.
    info: CogInfo,
    /// Levels indexed by `zoom - info.min_zoom`.
    levels: Vec<Option<Level>>,
    /// Why images that cannot be read as tiles were left out.
    skipped: Vec<SkippedImage>,
    /// How stored tiles are converted to output tiles.
    encoding: Encoding,
    /// Decoders for compressed tiles.
    decoders: Arc<DecoderRegistry>,
    /// Identifies this opened file in `bands`.
    file_id: FileId,
    /// Cached row bands of striped images, shared across a mosaic.
    bands: BandCache,
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
        Self::open(backend, prefetch, None, BandCache::default()).await
    }

    /// Opens a mosaic file, placing it on `tile` if it has no `GeoTIFF` tags.
    #[cfg(feature = "mosaic")]
    pub(crate) async fn try_from_backend_on_tile(backend: B, tile: TileCoord, bands: BandCache) -> CogResult<Self> {
        Self::open(backend, DEFAULT_PREFETCH, Some(tile), bands).await
    }

    /// Shared implementation of the constructors.
    async fn open(backend: B, prefetch: u64, untagged_tile: Option<TileCoord>, bands: BandCache) -> CogResult<Self> {
        let bridge = TiffBridge(Arc::new(backend));
        let ifds = read_ifds(&bridge, prefetch).await?;
        let location = bridge.0.location();

        let base = ifds
            .first()
            .ok_or_else(|| CogError::unsupported(location, UnsupportedReason::NoImages))?;
        let georef = match untagged_tile {
            Some(tile) if base.geo_key_directory().is_none() => Georef::from_tile(tile, base, location)?,
            _ => Georef::from_ifd(base, location)?,
        };
        let (format, encoding) = choose_encoding(base, location)?;
        let base_chunks = Chunks::of(base, 0, location)?;
        let tile_size = base_chunks.tile_size();
        let width = f64::from(base.image_width()) * georef.resolution;
        let height = f64::from(base.image_height()) * georef.resolution;
        let mut chunks = Vec::with_capacity(ifds.len());
        let mut previous = None;
        for (index, ifd) in ifds.iter().enumerate() {
            chunks.push(match is_image(index, ifd, previous) {
                Ok(true) => {
                    previous = Some(ifd);
                    Some(Ok(check_same_layout(base, base_chunks, ifd, index, location)?))
                }
                Ok(false) => None,
                Err(skipped) => Some(Err(skipped)),
            });
        }

        // Images that do not match a zoom level or are off the tile grid are left out, so a file
        // whose overviews are misaligned still provides the levels that are aligned.
        let mut levels: Vec<Level> = Vec::new();
        let mut skipped = Vec::new();
        for (index, (ifd, chunks)) in ifds.into_iter().zip(chunks).enumerate() {
            let chunks = match chunks {
                Some(Ok(chunks)) => chunks,
                Some(Err(reason)) => {
                    skipped.push(reason);
                    continue;
                }
                None => continue,
            };
            match make_level(ifd, chunks, index, &georef, width) {
                Ok(level) if levels.iter().any(|l| l.zoom == level.zoom) => {
                    skipped.push(SkippedImage::DuplicateZoom {
                        index,
                        zoom: level.zoom,
                    });
                }
                Ok(level) => levels.push(level),
                Err(reason) => skipped.push(reason),
            }
        }
        let Some(max_zoom) = levels.iter().map(|l| l.zoom).max() else {
            return Err(CogError::unsupported(
                location,
                if skipped.is_empty() {
                    UnsupportedReason::NoImages
                } else {
                    UnsupportedReason::NoUsableImages(skipped)
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
            decoders: Arc::new(DecoderRegistry::default()),
            file_id: FileId::next(),
            bands,
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
    pub fn skipped_images(&self) -> &[SkippedImage] {
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
        match self.encoding {
            Encoding::Passthrough => {
                let Some(tile) = self.locate(coord)? else {
                    return Ok(None);
                };
                let bytes = self.bridge.0.read(tile.range).await?;
                let ifd = &tile.level.ifd;
                Ok(Some(match ifd.jpeg_tables() {
                    Some(tables) if ifd.compression() == Compression::ModernJPEG => {
                        crate::jpeg::merge_tables(tables, bytes)
                    }
                    _ => bytes,
                }))
            }
            #[cfg(feature = "png")]
            Encoding::Png => match self.get_tile_pixels(coord).await? {
                Some(pixels) => crate::encode::encode_png(&pixels, self.bridge.0.location()).map(Some),
                None => Ok(None),
            },
        }
    }

    /// Reads and decodes one tile into [`CogInfo::tile_size`]-square pixels, with the bands of the file.
    ///
    /// Returns `Ok(None)` like [`get_tile`](Self::get_tile). Decoding happens on the calling task,
    /// so spawn concurrent calls as tasks to decode on several threads.
    /// WebP tiles need the `webp` feature.
    ///
    /// # Errors
    /// Returns an error if the backend fails or the tile cannot be decoded.
    pub async fn get_tile_pixels(&self, coord: TileCoord) -> CogResult<Option<TilePixels>> {
        if let Some(level) = self.level(coord.z())
            && let Chunks::Strips(strips) = level.chunks
        {
            let Some((col, row)) = level.position(coord) else {
                return Ok(None);
            };
            return self.read_rows(level, col, row, strips).await.map(Some);
        }
        match self.locate(coord)? {
            Some(StoredTile { level, col, row, .. }) => self.decode(level, col, row).await.map(Some),
            None => Ok(None),
        }
    }

    /// Cuts tile `col`, `row` out of the full-width band of rows it lies in, read from the strips of `level`.
    async fn read_rows(&self, level: &Level, col: usize, row: usize, strips: Strips) -> CogResult<TilePixels> {
        let location = self.bridge.0.location();
        let ifd = &level.ifd;
        let size = u64::from(STRIPED_TILE_SIZE);
        let samples = u64::from(ifd.samples_per_pixel());
        let left = col as u64 * size * samples;
        let top = row as u64 * size;
        let cols = size.min(u64::from(ifd.image_width()) - col as u64 * size) * samples;
        let rows = size.min(u64::from(ifd.image_height()) - top);
        let key = BandKey {
            file: self.file_id,
            zoom: level.zoom,
            row: u32::try_from(row).expect("tile rows fit in u32"),
        };
        let read_band = async {
            let offsets = ifd.strip_offsets().expect("strip offsets are checked when opening");
            let ranges = strips.ranges(offsets, top..top + rows);
            let mut parts = self.bridge.0.read_ranges(ranges).await?;
            if parts.len() == 1 {
                return Ok(parts.remove(0));
            }
            Ok(Bytes::from(parts.concat()))
        };
        let band = self.bands.get(key, location, read_band).await?;
        let to_usize = |v: u64| usize::try_from(v).expect("band offsets fit in usize");
        let mut data = Vec::with_capacity(to_usize(rows * cols));
        for y in 0..rows {
            let start = to_usize(y * strips.row_bytes + left);
            data.extend_from_slice(&band[start..start + to_usize(cols)]);
        }
        let shape = [to_usize(rows), to_usize(cols / samples), to_usize(samples)];
        TilePixels::padded(data, shape, STRIPED_TILE_SIZE, location)
    }

    /// The image, internal tile and byte range holding `coord`, or `None` if nothing is stored there.
    fn locate(&self, coord: TileCoord) -> CogResult<Option<StoredTile<'_>>> {
        let Some(level) = self.level(coord.z()) else {
            return Ok(None);
        };
        let Some((col, row)) = level.position(coord) else {
            return Ok(None);
        };
        let range = match level.ifd.tile_byte_range(col, row) {
            Some(TileByteRange::Chunky(range)) => range,
            Some(TileByteRange::Planar(ranges)) if ranges.len() == 1 => ranges[0].clone(),
            _ => {
                return Err(CogError::unsupported(
                    self.bridge.0.location(),
                    UnsupportedReason::MissingByteRange {
                        zoom: level.zoom,
                        col,
                        row,
                    },
                ));
            }
        };
        Ok((!range.is_empty()).then_some(StoredTile { level, col, row, range }))
    }

    /// Reads and decodes internal tile `col`, `row` of `level`.
    async fn decode(&self, level: &Level, col: usize, row: usize) -> CogResult<TilePixels> {
        let location = self.bridge.0.location();
        let tile = level
            .ifd
            .fetch_tile(col, row, &self.bridge)
            .await
            .map_err(|e| unwrap_tiff_error(location, e))?;
        let (data, shape, _) = tile
            .decode(&self.decoders)
            .map_err(|e| CogError::tiff(location, e))?
            .into_inner();
        let async_tiff::TypedArray::UInt8(data) = data else {
            return Err(CogError::unsupported(location, UnsupportedReason::DecodedNotEightBit));
        };
        TilePixels::padded(data, shape, self.info.tile_size, location)
    }

    /// Image for `zoom`, if any.
    fn level(&self, zoom: u8) -> Option<&Level> {
        let index = zoom.checked_sub(self.info.min_zoom)?;
        self.levels.get(usize::from(index))?.as_ref()
    }
}

/// Reads all IFDs, converting `async-tiff` panics on malformed input into errors.
pub(crate) async fn read_ifds<B: AsyncBackend>(
    bridge: &TiffBridge<B>,
    prefetch: u64,
) -> CogResult<Vec<ImageFileDirectory>> {
    let location = bridge.0.location();
    let cache = BlockMetadataCache::new(bridge.clone(), prefetch.min(bridge.0.size()));
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
                UnsupportedReason::MalformedMetadata(reason.to_owned()),
            ))
        }
    }
}

/// Recovers our own backend error if `async-tiff` wrapped it, otherwise wraps the TIFF error.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "every other `async-tiff` error is wrapped as it is"
)]
pub(crate) fn unwrap_tiff_error(location: &str, error: async_tiff::error::AsyncTiffError) -> CogError {
    match error {
        async_tiff::error::AsyncTiffError::External(inner) => match inner.downcast::<CogError>() {
            Ok(cog) => *cog,
            Err(other) => CogError::backend(location, other),
        },
        other => CogError::tiff(location, other),
    }
}

/// Picks the output format for the full-resolution image's pixel layout.
fn choose_encoding(ifd: &ImageFileDirectory, location: &str) -> CogResult<(TileFormat, Encoding)> {
    let samples = ifd.samples_per_pixel();
    let photometric = ifd.photometric_interpretation();
    let eight_bit =
        ifd.bits_per_sample().iter().all(|&b| b == 8) && ifd.sample_format().iter().all(|&f| f == SampleFormat::Uint);
    if !eight_bit {
        return Err(CogError::unsupported(
            location,
            UnsupportedReason::SampleType {
                bits: ifd.bits_per_sample().to_vec(),
                formats: ifd.sample_format().to_vec(),
            },
        ));
    }
    if samples > 1 && ifd.planar_configuration() != PlanarConfiguration::Chunky {
        return Err(CogError::unsupported(location, UnsupportedReason::PlanarBands));
    }
    #[cfg(feature = "png")]
    let gray = photometric == PhotometricInterpretation::BlackIsZero && matches!(samples, 1 | 2);
    #[cfg(feature = "png")]
    let rgb = photometric == PhotometricInterpretation::RGB && matches!(samples, 3 | 4);

    let compression = ifd.compression();
    match compression {
        Compression::WebP if matches!(samples, 3 | 4) => Ok((TileFormat::Webp, Encoding::Passthrough)),
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
        Compression::None | Compression::Deflate | Compression::OldDeflate | Compression::LZW | Compression::ZSTD
            if gray || rgb =>
        {
            Ok((TileFormat::Png, Encoding::Png))
        }
        _ => Err(CogError::unsupported(
            location,
            UnsupportedReason::PixelLayout {
                compression,
                samples,
                photometric,
            },
        )),
    }
}

/// Returns whether an IFD is the full-resolution image or an overview, or why a page was skipped.
pub(crate) fn is_image(
    index: usize,
    ifd: &ImageFileDirectory,
    previous: Option<&ImageFileDirectory>,
) -> Result<bool, SkippedImage> {
    const REDUCED_RESOLUTION: u32 = 0b001;
    const PAGE: u32 = 0b010;
    const MASK: u32 = 0b100;
    let subfile = ifd.new_subfile_type().unwrap_or(0);
    if subfile & MASK != 0 {
        return Ok(false);
    }
    if index == 0 || subfile & REDUCED_RESOLUTION != 0 {
        return Ok(true);
    }
    if subfile & PAGE == 0 {
        return Ok(false);
    }
    let Some(previous) = previous else {
        return Ok(false);
    };
    let (width, height) = (ifd.image_width(), ifd.image_height());
    let (half_width, half_height) = (previous.image_width().div_ceil(2), previous.image_height().div_ceil(2));
    if (width, height) != (half_width, half_height) {
        return Err(SkippedImage::PageNotHalf {
            index,
            width,
            height,
            half_width,
            half_height,
        });
    }
    Ok(true)
}

/// Reads whether an image is tiled or striped, without validating it.
fn chunking(ifd: &ImageFileDirectory) -> Chunking {
    match (ifd.tile_width(), ifd.tile_height()) {
        (Some(width), Some(height)) => Chunking::Tiles { width, height },
        _ => Chunking::Strips,
    }
}

/// Validates an image's pixel order and storage against the full-resolution image, and returns its chunking.
pub(crate) fn check_same_layout(
    base: &ImageFileDirectory,
    base_chunks: Chunks,
    ifd: &ImageFileDirectory,
    index: usize,
    location: &str,
) -> CogResult<Chunks> {
    let unsupported = |reason| CogError::unsupported(location, reason);
    if let Some(orientation) = ifd.orientation().filter(|&orientation| orientation != 1) {
        return Err(unsupported(UnsupportedReason::Orientation { index, orientation }));
    }
    let fill_order = ifd
        .other_tags()
        .get(&Tag::FillOrder)
        .and_then(|value| value.clone().into_u16().ok());
    if let Some(fill_order) = fill_order.filter(|&fill_order| fill_order != 1) {
        return Err(unsupported(UnsupportedReason::FillOrder { index, fill_order }));
    }
    if index == 0 {
        return Ok(base_chunks);
    }
    if chunking(ifd) != chunking(base) {
        return Err(unsupported(UnsupportedReason::OverviewChunking {
            index,
            overview: chunking(ifd),
            base: chunking(base),
        }));
    }
    let chunks = Chunks::of(ifd, index, location)?;
    let values = [
        (
            Tag::Compression,
            format!("{:?}", ifd.compression()),
            format!("{:?}", base.compression()),
        ),
        (
            Tag::PhotometricInterpretation,
            format!("{:?}", ifd.photometric_interpretation()),
            format!("{:?}", base.photometric_interpretation()),
        ),
        (
            Tag::SamplesPerPixel,
            ifd.samples_per_pixel().to_string(),
            base.samples_per_pixel().to_string(),
        ),
        (
            Tag::BitsPerSample,
            format!("{:?}", ifd.bits_per_sample()),
            format!("{:?}", base.bits_per_sample()),
        ),
        (
            Tag::PlanarConfiguration,
            format!("{:?}", ifd.planar_configuration()),
            format!("{:?}", base.planar_configuration()),
        ),
    ];
    if let Some((tag, overview, base)) = values.into_iter().find(|(_, overview, base)| overview != base) {
        return Err(unsupported(UnsupportedReason::OverviewLayout {
            index,
            tag,
            overview,
            base,
        }));
    }
    Ok(chunks)
}

/// Maps an image to its zoom level, or returns why it is skipped.
fn make_level(
    ifd: ImageFileDirectory,
    chunks: Chunks,
    index: usize,
    georef: &Georef,
    base_width: f64,
) -> Result<Level, SkippedImage> {
    let tile_size = chunks.tile_size();
    let resolution = if index == 0 {
        georef.resolution
    } else {
        base_width / f64::from(ifd.image_width())
    };
    let Some(zoom) = match_zoom(resolution, tile_size) else {
        return Err(SkippedImage::NoMatchingZoom {
            index,
            resolution,
            tile_size,
        });
    };
    let Some(first_tile) = aligned_tile(georef.origin_x, georef.origin_y, zoom, tile_size) else {
        return Err(SkippedImage::Misaligned { index, zoom });
    };
    let tile_count = (
        ifd.image_width().div_ceil(tile_size),
        ifd.image_height().div_ceil(tile_size),
    );
    let world = 1_u64 << zoom;
    if u64::from(first_tile.0) + u64::from(tile_count.0) > world
        || u64::from(first_tile.1) + u64::from(tile_count.1) > world
    {
        return Err(SkippedImage::BeyondWorld { index, zoom });
    }
    let (first_col, first_row) = first_tile;
    let (col_count, row_count) = tile_count;
    Ok(Level {
        zoom,
        cols: first_col..first_col + col_count,
        rows: first_row..first_row + row_count,
        chunks,
        ifd,
    })
}

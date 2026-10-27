//! COGs in any supported projection and on any pixel grid, resampled onto `WebMercatorQuad` tiles.

use std::sync::Arc;
use std::time::Duration;

use async_tiff::ImageFileDirectory;
use async_tiff::decoder::DecoderRegistry;
use async_tiff::tags::{PlanarConfiguration, SampleFormat};
use futures::future::try_join_all;
use moka::future::Cache;

use crate::backend::{AsyncBackend, TiffBridge};
use crate::bands::{FileId, SharedError};
use crate::crs::Crs;
use crate::geo::{PixelTransform, projected_type};
use crate::reader::{Chunks, check_same_layout, is_image, read_ifds, unwrap_tiff_error};
use crate::tile::{EARTH_CIRCUMFERENCE, HALF_EARTH, MAX_ZOOM, TileCoord, mercator_to_lnglat, resolution};
use crate::{CogError, CogResult, DEFAULT_PREFETCH, TilePixels, UnsupportedReason};

/// Output pixels between the points projected exactly; the pixels in between are interpolated.
const GRID_STEP: u32 = 16;

/// Maximum size of all cached decoded blocks.
const BLOCK_CACHE_BYTES: u64 = 256 * 1024 * 1024;

/// How long a decoded block is kept without being used.
const BLOCK_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Degrees between the two points whose distance gives a file's scale relative to Web Mercator.
const SCALE_PROBE_DEGREES: f64 = 1e-3;

/// Cache key of one decoded block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct BlockKey {
    /// File the block belongs to.
    file: FileId,
    /// Index of the image in [`WarpedCog::levels`].
    level: usize,
    /// Internal tile column.
    col: usize,
    /// Internal tile row.
    row: usize,
}

/// Size-bounded cache of decoded blocks, where concurrent reads of one block share one fetch.
/// `None` is a block the file does not store.
#[derive(Debug, Clone)]
pub(crate) struct BlockCache(Cache<BlockKey, Option<Arc<TilePixels>>>);

impl Default for BlockCache {
    fn default() -> Self {
        Self(
            Cache::builder()
                .max_capacity(BLOCK_CACHE_BYTES)
                .weigher(|_key, block: &Option<Arc<TilePixels>>| {
                    block
                        .as_ref()
                        .map_or(1, |pixels| u32::try_from(pixels.data().len()).unwrap_or(u32::MAX))
                })
                .time_to_idle(BLOCK_IDLE_TIMEOUT)
                .build(),
        )
    }
}

/// One image (full resolution or overview) of a warped COG.
#[derive(Debug)]
struct Level {
    /// Image metadata.
    ifd: ImageFileDirectory,
    /// Width in pixels.
    width: u32,
    /// Height in pixels.
    height: u32,
    /// Internal tile size in pixels.
    block_size: u32,
    /// Full-resolution pixels per pixel of this image, horizontally.
    scale_x: f64,
    /// Full-resolution pixels per pixel of this image, vertically.
    scale_y: f64,
}

/// A tile being painted from several files: the selected bands of each pixel, and whether it is set yet.
#[derive(Debug)]
pub(crate) struct Canvas {
    /// Width and height in pixels.
    size: u32,
    /// Bands per pixel, without alpha.
    bands: usize,
    /// `size * size` pixels of `bands + 1` samples, the last one 255 once the pixel is painted.
    data: Vec<u8>,
    /// Pixels not painted yet.
    empty: usize,
}

impl Canvas {
    /// An empty `size`-pixel square canvas of `bands` bands.
    pub fn new(size: u32, bands: usize) -> Self {
        let pixels = size as usize * size as usize;
        Self {
            size,
            bands,
            data: vec![0; pixels * (bands + 1)],
            empty: pixels,
        }
    }

    /// Whether every pixel is painted.
    pub fn is_full(&self) -> bool {
        self.empty == 0
    }

    /// Whether no pixel is painted.
    pub fn is_empty(&self) -> bool {
        self.empty == self.size as usize * self.size as usize
    }

    /// The painted pixels with an alpha band: opaque where painted, transparent elsewhere.
    pub fn into_pixels(self, location: &str) -> CogResult<TilePixels> {
        let size = self.size as usize;
        TilePixels::padded(self.data, [size, size, self.bands + 1], self.size, location)
    }
}

/// Reads tiles from one COG in any supported projection, resampling with nearest neighbour.
///
/// Every image must be tiled, with 8-bit unsigned samples stored pixel-interleaved.
/// A pixel whose selected bands all equal the nodata value is left unpainted.
#[derive(Debug)]
pub(crate) struct WarpedCog<B> {
    /// Backend wrapped for `async-tiff`.
    bridge: TiffBridge<B>,
    /// Coordinate system of the file.
    crs: Crs,
    /// Where the full-resolution image sits in `crs`.
    transform: PixelTransform,
    /// The full-resolution image, then its overviews from finest to coarsest.
    levels: Vec<Level>,
    /// Samples per pixel.
    samples: usize,
    /// The sample value of pixels without data, if any.
    nodata: Option<u8>,
    /// Decoders for compressed blocks.
    decoders: Arc<DecoderRegistry>,
    /// Identifies this opened file in `blocks`.
    file_id: FileId,
    /// Decoded blocks, shared by every file of a mosaic.
    blocks: BlockCache,
}

impl<B: AsyncBackend> WarpedCog<B> {
    /// Opens a COG, reading its headers and validating that it can be reprojected.
    ///
    /// `nodata` overrides the file's own `GDAL_NODATA` value.
    pub async fn open(backend: B, nodata: Option<u8>, blocks: BlockCache) -> CogResult<Self> {
        let bridge = TiffBridge(Arc::new(backend));
        let ifds = read_ifds(&bridge, DEFAULT_PREFETCH).await?;
        let location = bridge.0.location();
        let unsupported = |reason| CogError::unsupported(location, reason);

        let base = ifds.first().ok_or_else(|| unsupported(UnsupportedReason::NoImages))?;
        let epsg = projected_type(base, location)?;
        let crs = Crs::from_epsg(epsg).ok_or_else(|| unsupported(UnsupportedReason::UnprojectableCrs { epsg }))?;
        let transform = PixelTransform::from_ifd(base, location)?;
        check_samples(base, location)?;
        let nodata = nodata.or_else(|| base.gdal_nodata().and_then(parse_nodata));
        let samples = usize::from(base.samples_per_pixel());
        let base_chunks = Chunks::of(base, 0, location)?;
        let base_width = f64::from(base.image_width());
        let base_height = f64::from(base.image_height());

        let mut images = Vec::new();
        let mut previous = None;
        for (index, ifd) in ifds.iter().enumerate() {
            if !matches!(is_image(index, ifd, previous), Ok(true)) {
                continue;
            }
            let Chunks::Tiles(block_size) = check_same_layout(base, base_chunks, ifd, index, location)? else {
                return Err(unsupported(UnsupportedReason::StripedReprojection { index }));
            };
            images.push((index, block_size));
            previous = Some(ifd);
        }
        let mut levels = Vec::with_capacity(images.len());
        let mut images = images.into_iter().peekable();
        for (index, ifd) in ifds.into_iter().enumerate() {
            let Some((_, block_size)) = images.next_if(|&(image, _)| image == index) else {
                continue;
            };
            let (width, height) = (ifd.image_width(), ifd.image_height());
            levels.push(Level {
                width,
                height,
                block_size,
                scale_x: base_width / f64::from(width),
                scale_y: base_height / f64::from(height),
                ifd,
            });
        }
        Ok(Self {
            bridge,
            crs,
            transform,
            levels,
            samples,
            nodata,
            decoders: Arc::new(DecoderRegistry::default()),
            file_id: FileId::next(),
            blocks,
        })
    }

    /// Where the file is, for errors.
    pub fn location(&self) -> &str {
        self.bridge.0.location()
    }

    /// Samples per pixel.
    pub fn samples(&self) -> usize {
        self.samples
    }

    /// The lowest zoom whose `tile_size`-pixel tiles are at least as detailed as the full-resolution
    /// image at `[longitude, latitude]`.
    pub fn native_zoom(&self, [lng, lat]: [f64; 2], tile_size: u32) -> u8 {
        let [x0, y0] = self.crs.project(lng, lat);
        let [x1, y1] = self.crs.project(lng + SCALE_PROBE_DEGREES, lat);
        let mercator_meters = SCALE_PROBE_DEGREES / 180.0 * HALF_EARTH;
        let file_units_per_mercator_meter = (x1 - x0).hypot(y1 - y0) / mercator_meters;
        let pixel_in_mercator_meters = self.transform.scale_x / file_units_per_mercator_meter;
        let zoom = (EARTH_CIRCUMFERENCE / (f64::from(tile_size) * pixel_in_mercator_meters)).log2();
        // The zoom is a small positive number here, checked to be finite.
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        if zoom.is_finite() {
            (zoom - 0.01).ceil().clamp(0.0, f64::from(MAX_ZOOM)) as u8
        } else {
            MAX_ZOOM
        }
    }

    /// Paints the pixels of `coord` this file has data for onto the still-empty pixels of `canvas`,
    /// taking `bands` (counted from 0) of each.
    pub async fn paint(&self, coord: TileCoord, bands: &[usize], canvas: &mut Canvas) -> CogResult<()> {
        let grid = SourceGrid::new(self, coord, canvas.size);
        let level_index = self.level_for(grid.scale());
        let level = &self.levels[level_index];
        let Some([col_range, row_range]) = grid.blocks(level) else {
            return Ok(());
        };
        let reads = row_range
            .clone()
            .flat_map(|row| col_range.clone().map(move |col| (col, row)))
            .map(|(col, row)| self.block(level_index, col, row));
        let blocks = try_join_all(reads).await?;
        let cols = col_range.len();

        let out_samples = canvas.bands + 1;
        let samples = self.samples;
        for j in 0..canvas.size {
            for i in 0..canvas.size {
                let out = (j as usize * canvas.size as usize + i as usize) * out_samples;
                if canvas.data[out + canvas.bands] != 0 {
                    continue;
                }
                let [x, y] = grid.at(i, j);
                let (x, y) = (x / level.scale_x, y / level.scale_y);
                if !(x >= 0.0 && y >= 0.0) {
                    continue;
                }
                if x >= f64::from(level.width) || y >= f64::from(level.height) {
                    continue;
                }
                // Both are within the image here, so they fit in a u32.
                #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let (x, y) = (x as u32, y as u32);
                let (col, row) = ((x / level.block_size) as usize, (y / level.block_size) as usize);
                let Some(block) = &blocks[(row - row_range.start) * cols + (col - col_range.start)] else {
                    continue;
                };
                let (block_x, block_y) = ((x % level.block_size) as usize, (y % level.block_size) as usize);
                let inside = (block_y * level.block_size as usize + block_x) * samples;
                let pixel = &block.data()[inside..inside + samples];
                if self
                    .nodata
                    .is_some_and(|nodata| bands.iter().all(|&band| pixel[band] == nodata))
                {
                    continue;
                }
                for (target, &band) in canvas.data[out..out + canvas.bands].iter_mut().zip(bands) {
                    *target = pixel[band];
                }
                canvas.data[out + canvas.bands] = u8::MAX;
                canvas.empty -= 1;
            }
        }
        Ok(())
    }

    /// The coarsest image whose pixels are still at least as small as `scale` full-resolution pixels.
    fn level_for(&self, scale: f64) -> usize {
        self.levels
            .iter()
            .rposition(|level| level.scale_x <= scale * (1.0 + 1e-6))
            .unwrap_or(0)
    }

    /// Reads and decodes internal tile `col`, `row` of image `level`, or `None` if it is not stored.
    async fn block(&self, level: usize, col: usize, row: usize) -> CogResult<Option<Arc<TilePixels>>> {
        let key = BlockKey {
            file: self.file_id,
            level,
            col,
            row,
        };
        let location = self.location();
        self.blocks
            .0
            .try_get_with(key, self.decode(level, col, row))
            .await
            .map_err(|shared| match Arc::try_unwrap(shared) {
                Ok(error) => error,
                Err(shared) => CogError::backend(location, SharedError(shared)),
            })
    }

    /// Fetches and decodes one block.
    async fn decode(&self, level: usize, col: usize, row: usize) -> CogResult<Option<Arc<TilePixels>>> {
        let location = self.location();
        let level = &self.levels[level];
        let stored = match level.ifd.tile_byte_range(col, row) {
            Some(async_tiff::TileByteRange::Chunky(range)) => !range.is_empty(),
            _ => false,
        };
        if !stored {
            return Ok(None);
        }
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
        Ok(Some(Arc::new(TilePixels::padded(
            data,
            shape,
            level.block_size,
            location,
        )?)))
    }
}

/// Checks that the file has 8-bit unsigned samples stored pixel-interleaved.
fn check_samples(ifd: &ImageFileDirectory, location: &str) -> CogResult<()> {
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
    if ifd.samples_per_pixel() > 1 && ifd.planar_configuration() != PlanarConfiguration::Chunky {
        return Err(CogError::unsupported(location, UnsupportedReason::PlanarBands));
    }
    Ok(())
}

/// A `GDAL_NODATA` value that an 8-bit sample can hold.
fn parse_nodata(value: &str) -> Option<u8> {
    let value: f64 = value.trim().trim_end_matches('\0').parse().ok()?;
    // Checked to be a whole number within the range of a u8.
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    ((0.0..=255.0).contains(&value) && value.fract() == 0.0).then_some(value as u8)
}

/// Full-resolution pixel coordinates of a tile's pixel grid, projected exactly every
/// [`GRID_STEP`] pixels and interpolated in between.
struct SourceGrid {
    /// Grid points per row and column.
    points: u32,
    /// `points * points` full-resolution pixel coordinates, row by row from the top.
    coords: Vec<[f64; 2]>,
}

impl SourceGrid {
    /// Projects the grid of `coord`'s `size`-pixel tile into `cog`'s full-resolution pixels.
    fn new<B>(cog: &WarpedCog<B>, coord: TileCoord, size: u32) -> Self {
        let points = size.div_ceil(GRID_STEP) + 1;
        let pixel = resolution(coord.z(), size);
        let [min_x, _, _, max_y] = coord.mercator_bounds();
        let transform = cog.transform;
        let mut coords = Vec::with_capacity(points as usize * points as usize);
        for row in 0..points {
            for col in 0..points {
                let u = f64::from((col * GRID_STEP).min(size));
                let v = f64::from((row * GRID_STEP).min(size));
                let [lng, lat] = mercator_to_lnglat(u.mul_add(pixel, min_x), v.mul_add(-pixel, max_y));
                let [x, y] = cog.crs.project(lng, lat);
                coords.push([
                    (x - transform.origin_x) / transform.scale_x,
                    (transform.origin_y - y) / transform.scale_y,
                ]);
            }
        }
        Self { points, coords }
    }

    /// Full-resolution pixels per output pixel, the larger of both directions at the tile's center.
    fn scale(&self) -> f64 {
        let mid = self.points / 2;
        let [x, y] = self.point(mid - 1, mid - 1);
        let [xr, yr] = self.point(mid, mid - 1);
        let [xd, yd] = self.point(mid - 1, mid);
        let step = f64::from(GRID_STEP);
        ((xr - x).hypot(yr - y) / step).max((xd - x).hypot(yd - y) / step)
    }

    /// The source coordinate of output pixel `i`, `j` (its center), in full-resolution pixels.
    fn at(&self, i: u32, j: u32) -> [f64; 2] {
        let (col, row) = (i / GRID_STEP, j / GRID_STEP);
        let fx = (f64::from(i % GRID_STEP) + 0.5) / f64::from(GRID_STEP);
        let fy = (f64::from(j % GRID_STEP) + 0.5) / f64::from(GRID_STEP);
        let [x00, y00] = self.point(col, row);
        let [x10, y10] = self.point(col + 1, row);
        let [x01, y01] = self.point(col, row + 1);
        let [x11, y11] = self.point(col + 1, row + 1);
        let lerp = |a: f64, b: f64, t: f64| t.mul_add(b - a, a);
        [
            lerp(lerp(x00, x10, fx), lerp(x01, x11, fx), fy),
            lerp(lerp(y00, y10, fx), lerp(y01, y11, fx), fy),
        ]
    }

    /// The grid point at `col`, `row`.
    fn point(&self, col: u32, row: u32) -> [f64; 2] {
        self.coords[row as usize * self.points as usize + col as usize]
    }

    /// The internal tile columns and rows of `level` the grid touches, or `None` if it misses the image.
    fn blocks(&self, level: &Level) -> Option<[std::ops::Range<usize>; 2]> {
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for &[x, y] in &self.coords {
            min_x = min_x.min(x / level.scale_x);
            max_x = max_x.max(x / level.scale_x);
            min_y = min_y.min(y / level.scale_y);
            max_y = max_y.max(y / level.scale_y);
        }
        let range = |min: f64, max: f64, len: u32| {
            let block = f64::from(level.block_size);
            let blocks = f64::from(len.div_ceil(level.block_size));
            let first = (min / block).floor().max(0.0);
            let last = (max / block).floor().min(blocks - 1.0);
            // Both are clamped to the image's block range.
            #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            (first <= last && max >= 0.0 && min < f64::from(len)).then(|| first as usize..last as usize + 1)
        };
        Some([range(min_x, max_x, level.width)?, range(min_y, max_y, level.height)?])
    }
}

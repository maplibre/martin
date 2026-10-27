//! Decoded tiles and windows of pixels stitched from them.

use crate::tile::{HALF_EARTH, MAX_ZOOM, TileCoord, lnglat_to_mercator, resolution};
use crate::{CogError, CogResult, UnsupportedReason};

/// How close (as a fraction of a pixel) a bound may be to a pixel edge to count as on it.
const SNAP_TOLERANCE_PX: f64 = 1e-3;

/// One decoded tile: `size` x `size` pixels of `bands` interleaved 8-bit samples, row by row from the top.
///
/// The bands are those of the file: 1 (gray), 2 (gray + alpha), 3 (RGB, also for `YCbCr` JPEG) or 4 (RGBA).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TilePixels {
    data: Vec<u8>,
    size: u32,
    bands: usize,
}

impl TilePixels {
    /// Wraps a decoded tile of `[height, width, bands]`, padding a partial edge tile with zeros to `size`.
    pub(crate) fn padded(
        data: Vec<u8>,
        [height, width, bands]: [usize; 3],
        size: u32,
        location: &str,
    ) -> CogResult<Self> {
        let tile_px = size as usize;
        if bands == 0 || data.len() != height * width * bands || height > tile_px || width > tile_px {
            return Err(CogError::unsupported(
                location,
                UnsupportedReason::DecodedSize {
                    width,
                    height,
                    samples: bands,
                    size: tile_px,
                },
            ));
        }
        let data = if height == tile_px && width == tile_px {
            data
        } else {
            let mut buf = vec![0; tile_px * tile_px * bands];
            for (row, src) in data.chunks_exact(width * bands).enumerate() {
                let start = row * tile_px * bands;
                buf[start..start + src.len()].copy_from_slice(src);
            }
            buf
        };
        Ok(Self { data, size, bands })
    }

    /// The samples, `size * size * bands` bytes.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// The samples, without copying.
    #[must_use]
    pub fn into_data(self) -> Vec<u8> {
        self.data
    }

    /// Width and height in pixels.
    #[must_use]
    pub fn size(&self) -> u32 {
        self.size
    }

    /// Samples per pixel.
    #[must_use]
    pub fn bands(&self) -> usize {
        self.bands
    }
}

/// A rectangle of whole pixels at one zoom, on the pixel grid of `tile_size`-pixel `WebMercatorQuad` tiles.
///
/// It maps an area to the tiles it touches, and those tiles' pixels into one buffer:
///
/// ```
/// use cog_tiles::PixelWindow;
///
/// let window = PixelWindow::from_lnglat_bounds(16, 256, [-0.1, 51.49, -0.09, 51.5])?;
/// let bands = 3;
/// let mut raster = vec![0; window.width() as usize * window.height() as usize * bands];
/// for coord in window.tiles() {
///     // Fetch and decode `coord`, then `window.paste(&mut raster, coord, &pixels)`.
/// }
/// # Ok::<(), cog_tiles::CogError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelWindow {
    zoom: u8,
    tile_size: u32,
    /// The column of the leftmost pixel, counted from the west edge of the world.
    left: u64,
    /// The row of the top pixel, counted from the north edge of the world.
    top: u64,
    width: u32,
    height: u32,
}

impl PixelWindow {
    /// The pixels at `zoom` that overlap `[west, south, east, north]` in degrees.
    /// Latitudes beyond the Web Mercator limit (about ±85.05°) are clamped to it.
    ///
    /// # Errors
    /// Returns [`CogError::InvalidBounds`] if the bounds are not finite, `west >= east` or `south >= north`,
    /// and [`CogError::WindowTooLarge`] if the window is wider or taller than `u32::MAX` pixels.
    pub fn from_lnglat_bounds(zoom: u8, tile_size: u32, bounds: [f64; 4]) -> CogResult<Self> {
        let [west, south, east, north] = bounds;
        if !bounds.iter().all(|v| v.is_finite()) || west >= east || south >= north {
            return Err(CogError::InvalidBounds { bounds });
        }
        let [min_x, min_y] = lnglat_to_mercator(west, south);
        let [max_x, max_y] = lnglat_to_mercator(east, north);
        Self::from_mercator_bounds(zoom, tile_size, [min_x, min_y, max_x, max_y])
    }

    /// The pixels at `zoom` that overlap `[min_x, min_y, max_x, max_y]` in `EPSG:3857` meters,
    /// clamped to the world.
    ///
    /// # Errors
    /// Returns [`CogError::InvalidBounds`] if the bounds are not finite, empty, or outside the world,
    /// or `zoom` is above [`MAX_ZOOM`], and [`CogError::WindowTooLarge`] if the window is wider or
    /// taller than `u32::MAX` pixels.
    pub fn from_mercator_bounds(zoom: u8, tile_size: u32, bounds: [f64; 4]) -> CogResult<Self> {
        let [min_x, min_y, max_x, max_y] = bounds;
        if zoom > MAX_ZOOM || tile_size == 0 || !bounds.iter().all(|v| v.is_finite()) {
            return Err(CogError::InvalidBounds { bounds });
        }
        let res = resolution(zoom, tile_size);
        let world = (1_u64 << zoom) * u64::from(tile_size);
        // Both are clamped to 0..=world, which is at most 2^39 and exact in f64.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let to_pixel = |meters: f64, round: fn(f64) -> f64| {
            let pixel = meters / res;
            // Bounds converted from degrees land a rounding error past the pixel edge they name.
            let snapped = if (pixel - pixel.round()).abs() < SNAP_TOLERANCE_PX {
                pixel.round()
            } else {
                round(pixel)
            };
            snapped.clamp(0.0, world as f64) as u64
        };
        let left = to_pixel(min_x + HALF_EARTH, f64::floor);
        let right = to_pixel(max_x + HALF_EARTH, f64::ceil);
        let top = to_pixel(HALF_EARTH - max_y, f64::floor);
        let bottom = to_pixel(HALF_EARTH - min_y, f64::ceil);
        if left >= right || top >= bottom {
            return Err(CogError::InvalidBounds { bounds });
        }
        let too_large = |_err| CogError::WindowTooLarge {
            width: right - left,
            height: bottom - top,
        };
        Ok(Self {
            zoom,
            tile_size,
            left,
            top,
            width: u32::try_from(right - left).map_err(too_large)?,
            height: u32::try_from(bottom - top).map_err(too_large)?,
        })
    }

    /// The zoom level of the pixels.
    #[must_use]
    pub fn zoom(&self) -> u8 {
        self.zoom
    }

    /// Width in pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Meters per pixel.
    #[must_use]
    pub fn resolution(&self) -> f64 {
        resolution(self.zoom, self.tile_size)
    }

    /// The window's extent in `EPSG:3857` meters: `[min_x, min_y, max_x, max_y]`.
    #[must_use]
    #[expect(clippy::cast_precision_loss, reason = "pixel offsets are at most 2^39")]
    pub fn mercator_bounds(&self) -> [f64; 4] {
        let res = self.resolution();
        let min_x = (self.left as f64).mul_add(res, -HALF_EARTH);
        let max_y = (self.top as f64).mul_add(-res, HALF_EARTH);
        let max_x = f64::from(self.width).mul_add(res, min_x);
        let min_y = f64::from(self.height).mul_add(-res, max_y);
        [min_x, min_y, max_x, max_y]
    }

    /// The affine transform from pixel to `EPSG:3857`, in GDAL order:
    /// `[min_x, resolution, 0, max_y, 0, -resolution]`.
    #[must_use]
    pub fn geotransform(&self) -> [f64; 6] {
        let [min_x, _, _, max_y] = self.mercator_bounds();
        let res = self.resolution();
        [min_x, res, 0.0, max_y, 0.0, -res]
    }

    /// The tiles the window touches, row by row from the top.
    pub fn tiles(&self) -> impl Iterator<Item = TileCoord> + use<> {
        let size = u64::from(self.tile_size);
        let zoom = self.zoom;
        let to_tile = |pixel: u64| u32::try_from(pixel / size).unwrap_or(u32::MAX);
        let cols = to_tile(self.left)..=to_tile(self.left + u64::from(self.width) - 1);
        let rows = to_tile(self.top)..=to_tile(self.top + u64::from(self.height) - 1);
        rows.flat_map(move |y| cols.clone().map(move |x| TileCoord::new_unchecked(zoom, x, y)))
    }

    /// Copies the part of `tile` (the tile at `coord`) inside the window into `raster`,
    /// a `width * height * bands` buffer laid out like [`TilePixels`].
    /// Parts outside the window, and tiles of another zoom or tile size, are ignored.
    ///
    /// # Panics
    /// If `raster` is not `width * height * tile.bands()` bytes long.
    pub fn paste(&self, raster: &mut [u8], coord: TileCoord, tile: &TilePixels) {
        let bands = tile.bands();
        let (width, height) = (self.width as usize, self.height as usize);
        assert_eq!(
            raster.len(),
            width * height * bands,
            "the raster must be width * height * bands bytes"
        );
        if coord.z() != self.zoom || tile.size() != self.tile_size {
            return;
        }
        let size = u64::from(self.tile_size);
        let tile_left = u64::from(coord.x()) * size;
        let tile_top = u64::from(coord.y()) * size;
        let left = tile_left.max(self.left);
        let right = (tile_left + size).min(self.left + u64::from(self.width));
        let top = tile_top.max(self.top);
        let bottom = (tile_top + size).min(self.top + u64::from(self.height));
        if left >= right || top >= bottom {
            return;
        }
        // Every offset below is within one tile or the window, both at most u32::MAX pixels.
        #[expect(clippy::cast_possible_truncation)]
        let offset = |v: u64| v as usize;
        let span = offset(right - left) * bands;
        let tile_px = self.tile_size as usize;
        for row in top..bottom {
            let src = (offset(row - tile_top) * tile_px + offset(left - tile_left)) * bands;
            let dst = (offset(row - self.top) * width + offset(left - self.left)) * bands;
            raster[dst..dst + span].copy_from_slice(&tile.data[src..src + span]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(size: u32, bands: usize, value: impl Fn(usize, usize) -> u8) -> TilePixels {
        let tile_px = size as usize;
        let data = (0..tile_px * tile_px * bands)
            .map(|i| value(i / bands % tile_px, i / bands / tile_px))
            .collect();
        TilePixels { data, size, bands }
    }

    #[test]
    fn pads_partial_tiles() {
        let pixels = TilePixels::padded(vec![255; 2 * 3], [1, 2, 3], 4, "test").unwrap();
        assert_eq!(pixels.size(), 4);
        assert_eq!(&pixels.data()[..6], &[255; 6]);
        assert!(pixels.data()[6..].iter().all(|&v| v == 0));
        TilePixels::padded(vec![0; 3], [1, 1, 1], 1, "test").unwrap_err();
    }

    #[test]
    fn a_window_snaps_outward_to_whole_pixels() {
        let window = PixelWindow::from_mercator_bounds(1, 4, [-HALF_EARTH * 0.6, 0.1, -1.0, HALF_EARTH * 0.4]).unwrap();
        assert_eq!((window.width(), window.height()), (3, 2));
        let [min_x, min_y, max_x, max_y] = window.mercator_bounds();
        assert!((min_x - -HALF_EARTH * 0.75).abs() < 1e-6);
        assert!(min_y.abs() < 1e-6 && max_x.abs() < 1e-6);
        assert!((max_y - HALF_EARTH * 0.5).abs() < 1e-6);
        assert_eq!(
            window.geotransform(),
            [min_x, HALF_EARTH / 4.0, 0.0, max_y, 0.0, -HALF_EARTH / 4.0]
        );
        let tiles: Vec<_> = window.tiles().map(|t| t.to_string()).collect();
        assert_eq!(tiles, ["1/0/0"]);
    }

    #[test]
    fn a_tile_given_in_degrees_is_exactly_one_tile() {
        let tile = TileCoord::new(16, 32_748, 21_794).unwrap();
        let [min_x, min_y, max_x, max_y] = tile.mercator_bounds();
        let [west, south] = crate::mercator_to_lnglat(min_x, min_y);
        let [east, north] = crate::mercator_to_lnglat(max_x, max_y);
        let window = PixelWindow::from_lnglat_bounds(16, 256, [west, south, east, north]).unwrap();
        assert_eq!((window.width(), window.height()), (256, 256));
        assert_eq!(window.tiles().collect::<Vec<_>>(), [tile]);
    }

    #[test]
    fn invalid_windows_are_rejected() {
        PixelWindow::from_lnglat_bounds(3, 256, [10.0, 0.0, 5.0, 1.0]).unwrap_err();
        PixelWindow::from_lnglat_bounds(3, 256, [0.0, 1.0, 1.0, 1.0]).unwrap_err();
        PixelWindow::from_lnglat_bounds(3, 256, [f64::NAN, 0.0, 1.0, 1.0]).unwrap_err();
        PixelWindow::from_mercator_bounds(31, 256, [0.0, 0.0, 1.0, 1.0]).unwrap_err();
        PixelWindow::from_mercator_bounds(3, 256, [HALF_EARTH, 0.0, HALF_EARTH * 2.0, 1.0]).unwrap_err();
        let error = PixelWindow::from_lnglat_bounds(30, 512, [-180.0, -85.0, 180.0, 85.0]).unwrap_err();
        assert!(matches!(error, CogError::WindowTooLarge { .. }), "{error:?}");
    }

    #[test]
    fn pasting_copies_only_the_overlap() {
        // Pixels 3..7 x 2..5 at z1 with 4-px tiles: straddles all four tiles.
        let res = resolution(1, 4);
        let window = PixelWindow::from_mercator_bounds(
            1,
            4,
            [
                3.0f64.mul_add(res, -HALF_EARTH),
                HALF_EARTH - 5.0 * res,
                7.0f64.mul_add(res, -HALF_EARTH),
                HALF_EARTH - 2.0 * res,
            ],
        )
        .unwrap();
        assert_eq!((window.width(), window.height()), (4, 3));
        assert_eq!(window.tiles().count(), 4);
        let mut raster = vec![0; 4 * 3 * 2];
        for coord in window.tiles() {
            let id = u8::try_from(coord.y() * 2 + coord.x()).unwrap();
            window.paste(
                &mut raster,
                coord,
                &tile(4, 2, |x, y| id * 16 + u8::try_from(y * 4 + x).unwrap()),
            );
        }
        let first_band: Vec<u8> = raster.iter().step_by(2).copied().collect();
        assert_eq!(first_band, [11, 24, 25, 26, 15, 28, 29, 30, 35, 48, 49, 50]);
    }
}

//! Decoding nine fetched tiles and stitching them into one field.

#![expect(clippy::cast_possible_truncation)]

use image::RgbaImage;
use martin_tile_utils::TileData;

use crate::tiles::raster_codecs::ensure_jxl_decoding_hook;

/// Number of tiles in a neighbourhood.
pub const NEIGHBOURHOOD_LEN: usize = 3 * 3;

/// Tiles per side of the neighbourhood.
pub const GRID_SIDE: usize = 3;

/// Side length in pixels of one upstream tile.
pub const TILE_SIZE: usize = 256;

/// Side length in pixels of the assembled field.
pub const FIELD_SIDE: usize = GRID_SIDE * TILE_SIZE;

/// Channels in an assembled texel.
pub(crate) const CHANNELS: usize = 4;

/// Errors raised while assembling a neighbourhood.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum NeighbourhoodError {
    /// The centre tile was fetched successfully but could not be decoded as an image.
    #[error(
        "The centre tile was fetched but could not be decoded as an image. \
         The upstream source served malformed data for this tile."
    )]
    CorruptCentreTile,
}

/// A 3x3 block of fetched tiles, in row-major order, each slot either present or missing.
#[derive(Debug, Clone, Default)]
pub struct Neighbourhood {
    tiles: [Option<TileData>; NEIGHBOURHOOD_LEN],
}

impl Neighbourhood {
    /// Row-major index of the centre tile.
    pub const CENTRE: usize = 4;

    /// Tile offset `(dx, dy)` of row-major slot `index` from the centre.
    #[must_use]
    pub const fn offset(index: usize) -> (i32, i32) {
        #[expect(clippy::cast_possible_wrap)]
        ((index % GRID_SIDE) as i32 - 1, (index / GRID_SIDE) as i32 - 1)
    }

    /// Builds a neighbourhood from nine fetch results in row-major order.
    #[must_use]
    pub fn from_row_major(tiles: [Option<TileData>; NEIGHBOURHOOD_LEN]) -> Self {
        Self { tiles }
    }

    /// Builds a neighbourhood holding only a centre tile; the eight
    /// neighbours are edge-clamped from it.
    #[must_use]
    pub fn centre_only(centre: TileData) -> Self {
        let mut tiles: [Option<TileData>; NEIGHBOURHOOD_LEN] = Default::default();
        tiles[Self::CENTRE] = Some(centre);
        Self { tiles }
    }

    /// Decodes and stitches the nine slots into one [`FIELD_SIDE`]-square RGBA field.
    ///
    /// # Errors
    ///
    /// Returns [`NeighbourhoodError::CorruptCentreTile`] when a centre tile
    /// arrived but could not be decoded.
    pub fn assemble(&self) -> Result<RgbaField, NeighbourhoodError> {
        ensure_jxl_decoding_hook();
        let decode = |bytes: &TileData| -> Option<RgbaImage> {
            let img = image::load_from_memory(bytes).ok()?.into_rgba8();
            (img.width() > 0 && img.height() > 0).then_some(img)
        };
        let decoded: [Option<RgbaImage>; NEIGHBOURHOOD_LEN] =
            std::array::from_fn(|i| self.tiles[i].as_ref().and_then(decode));

        if self.tiles[Self::CENTRE].is_some() && decoded[Self::CENTRE].is_none() {
            return Err(NeighbourhoodError::CorruptCentreTile);
        }
        let centre = decoded[Self::CENTRE].as_ref();

        let mut rgba = vec![0u8; FIELD_SIDE * FIELD_SIDE * CHANNELS];
        for (i, slot) in decoded.iter().enumerate() {
            let grid = (i % GRID_SIDE, i / GRID_SIDE);
            for y in 0..TILE_SIZE {
                for x in 0..TILE_SIZE {
                    let px = resolve_pixel(slot.as_ref(), centre, grid, (x, y));
                    let base =
                        ((grid.1 * TILE_SIZE + y) * FIELD_SIDE + grid.0 * TILE_SIZE + x) * CHANNELS;
                    rgba[base..base + CHANNELS].copy_from_slice(&px);
                }
            }
        }
        Ok(RgbaField { rgba })
    }
}

/// A stitched 3x3 tile neighbourhood as one [`FIELD_SIDE`]-square RGBA buffer.
#[derive(Debug, Clone)]
pub struct RgbaField {
    rgba: Vec<u8>,
}

impl RgbaField {
    /// A field whose every texel is `texel`.
    #[must_use]
    pub fn uniform(texel: [u8; CHANNELS]) -> Self {
        Self {
            rgba: texel
                .iter()
                .copied()
                .cycle()
                .take(FIELD_SIDE * FIELD_SIDE * CHANNELS)
                .collect(),
        }
    }

    /// The raw texel at `(x, y)` in the assembled field.
    #[must_use]
    pub fn texel(&self, x: usize, y: usize) -> [u8; CHANNELS] {
        let base = (y * FIELD_SIDE + x) * CHANNELS;
        std::array::from_fn(|c| self.rgba[base + c])
    }

    /// Consumes the field and returns its row-major RGBA bytes.
    #[must_use]
    pub fn into_rgba(self) -> Vec<u8> {
        self.rgba
    }

    /// True when every texel in the field is fully zero.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.rgba.iter().all(|&b| b == 0)
    }
}

/// Reads a pixel from `img`, clamping the coordinates into its real extent.
///
/// Off-square tiles near the poles and the antimeridian mean a neighbourhood
/// can mix a e.g. 260x252 tile with 256-square ones; clamping squares them
/// all up to the field grid instead of failing on the shape mismatch.
#[inline]
fn clamped_pixel(img: &RgbaImage, x: usize, y: usize) -> [u8; CHANNELS] {
    let (w, h) = img.dimensions();
    let cx = x.min(w as usize - 1) as u32;
    let cy = y.min(h as usize - 1) as u32;
    img.get_pixel(cx, cy).0
}

/// Maps a coordinate in a missing neighbour to the centre coordinate that
/// should be replicated into it: `0` and `2` clamp to the centre's first or
/// last row/column, `1` (edge-adjacent) passes the coordinate through.
#[inline]
fn clamp_toward_centre(grid: usize, coord: usize) -> usize {
    match grid {
        0 => 0,
        2 => TILE_SIZE - 1,
        _ => coord,
    }
}

/// Resolves one field pixel at grid cell `(grid_x, grid_y)`, local
/// coordinate `(x, y)`: the slot's own pixel if it decoded, else clamped from
/// the centre, else blank.
#[inline]
fn resolve_pixel(
    slot: Option<&RgbaImage>,
    centre: Option<&RgbaImage>,
    (grid_x, grid_y): (usize, usize),
    (x, y): (usize, usize),
) -> [u8; CHANNELS] {
    if let Some(img) = slot {
        return clamped_pixel(img, x, y);
    }
    let Some(centre) = centre else {
        return [0; CHANNELS];
    };
    clamped_pixel(centre, clamp_toward_centre(grid_x, x), clamp_toward_centre(grid_y, y))
}

#[cfg(test)]
mod tests {
    use image::codecs::png::PngEncoder;
    use image::{ExtendedColorType, ImageEncoder as _};

    use super::*;

    /// PNG-encodes an image whose texel `(x, y)` is `[x, y, 0, 255]`, so a
    /// field position's origin is readable straight off the assembled field.
    pub(crate) fn positional_tile(width: usize, height: usize) -> TileData {
        let mut pixels = Vec::with_capacity(width * height * CHANNELS);
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&[x as u8, y as u8, 0, 255]);
            }
        }
        let mut buf = Vec::new();
        PngEncoder::new(&mut buf)
            .write_image(&pixels, width as u32, height as u32, ExtendedColorType::Rgba8)
            .expect("encode test tile");
        buf
    }

    /// JPEG XL-encodes the same positional image as [`positional_tile`].
    fn positional_tile_jxl(width: usize, height: usize) -> TileData {
        use zune_core::bit_depth::BitDepth;
        use zune_core::colorspace::ColorSpace;
        use zune_core::options::EncoderOptions;
        use zune_jpegxl::JxlSimpleEncoder;

        let mut pixels = Vec::with_capacity(width * height * CHANNELS);
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&[x as u8, y as u8, 0, 255]);
            }
        }
        let options = EncoderOptions::new(width, height, ColorSpace::RGBA, BitDepth::Eight);
        let mut buf = Vec::new();
        JxlSimpleEncoder::new(&pixels, options)
            .encode(&mut buf)
            .expect("encode test tile");
        buf
    }

    /// Field coordinate of local pixel `(x, y)` within grid cell `(gx, gy)`.
    fn at(gx: usize, gy: usize, x: usize, y: usize) -> (usize, usize) {
        (gx * TILE_SIZE + x, gy * TILE_SIZE + y)
    }

    #[test]
    fn missing_neighbours_replicate_the_centre_nearest_edge() {
        let tiles = Neighbourhood::centre_only(positional_tile(TILE_SIZE, TILE_SIZE));
        let field = tiles.assemble().expect("centre decodes");

        let (x, y) = at(1, 1, 40, 90);
        assert_eq!(field.texel(x, y), [40, 90, 0, 255], "centre verbatim");

        let (x, y) = at(1, 0, 40, 90);
        assert_eq!(field.texel(x, y), [40, 0, 0, 255], "north clamps rows");

        let (x, y) = at(0, 1, 40, 90);
        assert_eq!(field.texel(x, y), [0, 90, 0, 255], "west clamps columns");

        let (x, y) = at(2, 2, 40, 90);
        let last = (TILE_SIZE - 1) as u8;
        assert_eq!(field.texel(x, y), [last, last, 0, 255], "corner clamps both axes");
    }

    #[test]
    fn jxl_upstream_tiles_decode_too() {
        let tiles = Neighbourhood::centre_only(positional_tile_jxl(TILE_SIZE, TILE_SIZE));
        let field = tiles.assemble().expect("jxl centre decodes");

        let (x, y) = at(1, 1, 40, 90);
        assert_eq!(field.texel(x, y), [40, 90, 0, 255]);
    }

    #[test]
    fn off_size_edge_tiles_are_squared_up_to_the_tile_grid() {
        let tiles = Neighbourhood::from_row_major([
            None,
            Some(positional_tile(260, 252)),
            None,
            None,
            Some(positional_tile(TILE_SIZE, TILE_SIZE)),
            None,
            None,
            None,
            None,
        ]);
        let field = tiles.assemble().expect("off-size tiles square up");

        let (x, y) = at(1, 0, 255, 10);
        assert_eq!(field.texel(x, y), [255, 10, 0, 255], "overflow cropped");

        let (x, y) = at(1, 0, 100, 255);
        assert_eq!(field.texel(x, y), [100, 251, 0, 255], "shortfall edge-padded");
    }

    #[test]
    fn a_centre_that_arrived_but_will_not_decode_is_an_error() {
        let tiles = Neighbourhood::centre_only(b"this is not an image".to_vec());
        assert!(matches!(tiles.assemble(), Err(NeighbourhoodError::CorruptCentreTile)));
    }

    #[test]
    fn a_corrupt_neighbour_degrades_instead_of_failing() {
        let mut slots: [Option<TileData>; NEIGHBOURHOOD_LEN] = Default::default();
        slots[Neighbourhood::CENTRE] = Some(positional_tile(TILE_SIZE, TILE_SIZE));
        slots[1] = Some(b"garbage".to_vec());
        let field = Neighbourhood::from_row_major(slots)
            .assemble()
            .expect("a corrupt neighbour is survivable");

        let (x, y) = at(1, 0, 40, 90);
        assert_eq!(field.texel(x, y), [40, 0, 0, 255]);
    }

    #[test]
    fn an_entirely_absent_neighbourhood_assembles_blank() {
        let field = Neighbourhood::default()
            .assemble()
            .expect("an absent neighbourhood is not corrupt");
        assert!(field.is_blank());
    }

    #[test]
    fn offset_is_row_major_and_centred() {
        assert_eq!(Neighbourhood::offset(Neighbourhood::CENTRE), (0, 0));
        assert_eq!(Neighbourhood::offset(1), (0, -1));
        assert_eq!(Neighbourhood::offset(3), (-1, 0));
    }
}

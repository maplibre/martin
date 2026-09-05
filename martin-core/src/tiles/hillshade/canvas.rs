//! Builds the hillshade sampling canvas from a stitched tile neighbourhood.

use super::error::HillshadeError;
use super::shade::Canvas;
use crate::tiles::neighbourhood::Neighbourhood;

impl Canvas {
    /// Assembles `tiles` into the sampling field the bake reads.
    ///
    /// # Errors
    ///
    /// Returns [`HillshadeError::CorruptCentreTile`] when a centre tile
    /// arrived but could not be decoded.
    pub fn from_neighbourhood(tiles: &Neighbourhood) -> Result<Self, HillshadeError> {
        Ok(Self::from_rgba(tiles.assemble()?.into_rgba()))
    }
}

#[cfg(test)]
mod tests {
    use martin_tile_utils::TileData;

    use super::*;
    use crate::tiles::neighbourhood::{NEIGHBOURHOOD_LEN, TILE_SIZE};

    /// PNG-encodes an image whose texel `(x, y)` is `[x, y, 0, 255]`.
    fn positional_tile(width: usize, height: usize) -> TileData {
        use image::codecs::png::PngEncoder;
        use image::{ExtendedColorType, ImageEncoder as _};

        let mut pixels = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                #[expect(clippy::cast_possible_truncation)]
                pixels.extend_from_slice(&[x as u8, y as u8, 0, 255]);
            }
        }
        let mut buf = Vec::new();
        #[expect(clippy::cast_possible_truncation)]
        PngEncoder::new(&mut buf)
            .write_image(&pixels, width as u32, height as u32, ExtendedColorType::Rgba8)
            .expect("encode test tile");
        buf
    }

    #[test]
    fn the_canvas_carries_the_assembled_field_through() {
        let tiles = Neighbourhood::centre_only(positional_tile(TILE_SIZE, TILE_SIZE));
        let canvas = Canvas::from_neighbourhood(&tiles).expect("centre decodes");
        assert_eq!(canvas.raw_texel(TILE_SIZE + 40, TILE_SIZE + 90), [40, 90, 0, 255]);
    }

    #[test]
    fn a_centre_that_arrived_but_will_not_decode_is_an_error() {
        let tiles = Neighbourhood::centre_only(b"this is not an image".to_vec());
        assert!(matches!(
            Canvas::from_neighbourhood(&tiles),
            Err(HillshadeError::CorruptCentreTile)
        ));
    }

    #[test]
    fn an_entirely_absent_neighbourhood_bakes_from_a_blank_canvas() {
        let canvas = Canvas::from_neighbourhood(&Neighbourhood::default())
            .expect("an absent neighbourhood is not corrupt");
        assert!(canvas.is_blank());
    }

    #[test]
    fn a_corrupt_neighbour_degrades_instead_of_failing() {
        let mut slots: [Option<TileData>; NEIGHBOURHOOD_LEN] = Default::default();
        slots[Neighbourhood::CENTRE] = Some(positional_tile(TILE_SIZE, TILE_SIZE));
        slots[1] = Some(b"garbage".to_vec());
        let canvas = Canvas::from_neighbourhood(&Neighbourhood::from_row_major(slots))
            .expect("a corrupt neighbour is survivable");
        assert_eq!(canvas.raw_texel(TILE_SIZE + 40, 90), [40, 0, 0, 255]);
    }
}

//! Lossless encoding of a baked hillshade to an image format.

use martin_tile_utils::Format;

use super::error::HillshadeError;
use super::shade::BakedTile;

/// Image formats a baked hillshade can be encoded to.
pub const SUPPORTED_FORMATS: &[Format] = &[Format::Png, Format::Webp];

impl BakedTile {
    /// Encodes the baked grayscale image as `format`.
    pub fn encode(&self, format: Format) -> Result<Vec<u8>, HillshadeError> {
        use image::codecs::png::PngEncoder;
        use image::codecs::webp::WebPEncoder;
        use image::{ExtendedColorType, ImageEncoder as _};

        let expected = (self.side as usize) * (self.side as usize);
        if self.gray.len() != expected {
            return Err(HillshadeError::MalformedBake {
                actual: self.gray.len(),
                expected,
                side: self.side,
            });
        }

        let mut buf = Vec::new();
        let result = match format {
            Format::Png => PngEncoder::new(&mut buf).write_image(
                &self.gray,
                self.side,
                self.side,
                ExtendedColorType::L8,
            ),
            Format::Webp => WebPEncoder::new_lossless(&mut buf).write_image(
                &self.gray,
                self.side,
                self.side,
                ExtendedColorType::L8,
            ),
            other @ (Format::Gif
            | Format::Jpeg
            | Format::Json
            | Format::Mvt
            | Format::Mlt
            | Format::Avif) => return Err(HillshadeError::UnsupportedFormat(other)),
        };
        result.map_err(|source| HillshadeError::Encoding { format, source })?;
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use strum::IntoEnumIterator as _;

    use super::*;

    /// A small bake with a spread of tones, enough to catch a codec quantising.
    fn baked(side: u32) -> BakedTile {
        let gray = (0..side * side).map(|i| (i % 251) as u8).collect();
        BakedTile {
            side,
            gray,
            apron: 0,
        }
    }

    #[rstest]
    #[case::png(Format::Png)]
    #[case::webp(Format::Webp)]
    fn supported_formats_round_trip_losslessly(#[case] format: Format) {
        let tile = baked(64);
        let bytes = tile.encode(format).expect("encodes");
        let decoded = image::load_from_memory(&bytes).expect("decodes").to_luma8();

        assert_eq!(decoded.dimensions(), (tile.side, tile.side));
        assert_eq!(
            decoded.into_raw(),
            tile.gray,
            "{format:?} must not alter a single sample"
        );
    }

    #[test]
    fn lossy_and_vector_formats_are_rejected() {
        let tile = baked(16);
        for format in Format::iter().filter(|f| !SUPPORTED_FORMATS.contains(f)) {
            assert!(
                matches!(
                    tile.encode(format),
                    Err(HillshadeError::UnsupportedFormat(f)) if f == format
                ),
                "{format:?} must be rejected"
            );
        }
    }

    #[test]
    fn every_supported_format_actually_encodes() {
        let tile = baked(16);
        for &format in SUPPORTED_FORMATS {
            assert!(tile.encode(format).is_ok(), "{format:?} must encode");
        }
    }

    #[test]
    fn a_bake_inconsistent_with_its_dimensions_is_rejected() {
        let tile = BakedTile {
            side: 64,
            gray: vec![0; 10],
            apron: 0,
        };
        assert!(matches!(
            tile.encode(Format::Png),
            Err(HillshadeError::MalformedBake { actual: 10, .. })
        ));
    }
}

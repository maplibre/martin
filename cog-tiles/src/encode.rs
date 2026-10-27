//! Re-encoding decoded 8-bit tiles as PNG, WebP or JPEG.

use bytes::Bytes;

use crate::{CogError, CogResult, TileFormat, TilePixels, UnsupportedReason};

/// How decoded tiles are encoded, read as gray, gray and alpha, RGB, or RGB and alpha by band count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TileEncoding {
    /// Lossless PNG, fast to encode and large.
    #[default]
    Png,
    /// WebP, whose alpha stays lossless even when its colors are lossy.
    #[cfg(feature = "webp")]
    Webp {
        /// The quality from 1 (smallest) to 100 (best), or `None` for lossless.
        quality: Option<u8>,
    },
    /// JPEG, painted over `background` where tiles are transparent.
    #[cfg(feature = "jpeg")]
    Jpeg {
        /// The quality from 1 (smallest) to 100 (best).
        quality: u8,
        /// The RGB color shown where tiles are transparent, as JPEG has no alpha.
        background: [u8; 3],
    },
}

impl TileEncoding {
    /// The format of the tiles this encodes.
    #[must_use]
    pub fn format(self) -> TileFormat {
        match self {
            Self::Png => TileFormat::Png,
            #[cfg(feature = "webp")]
            Self::Webp { .. } => TileFormat::Webp,
            #[cfg(feature = "jpeg")]
            Self::Jpeg { .. } => TileFormat::Jpeg,
        }
    }

    /// Encodes one decoded tile on the calling task.
    ///
    /// # Errors
    /// Returns an error if `pixels` has more than four bands, or the encoder fails.
    pub fn encode(self, pixels: &TilePixels) -> CogResult<Bytes> {
        self.encode_at(pixels, "tile")
    }

    /// Encodes one decoded tile, naming `location` in errors.
    pub(crate) fn encode_at(self, pixels: &TilePixels, location: &str) -> CogResult<Bytes> {
        if !(1..=4).contains(&pixels.bands()) {
            return Err(CogError::unsupported(
                location,
                UnsupportedReason::EncodeSamples {
                    format: self.format(),
                    samples: pixels.bands(),
                },
            ));
        }
        let backend_error = |e: Box<dyn std::error::Error + Send + Sync>| CogError::Backend {
            location: location.to_owned(),
            source: e,
        };
        match self {
            Self::Png => encode_png(pixels).map_err(|e| backend_error(e.into())),
            #[cfg(feature = "webp")]
            Self::Webp { quality } => Ok(encode_webp(pixels, quality)),
            #[cfg(feature = "jpeg")]
            Self::Jpeg { quality, background } => {
                encode_jpeg(pixels, quality, background).map_err(|e| backend_error(e.into()))
            }
        }
    }
}

/// Encodes a tile of 1 to 4 bands as PNG.
fn encode_png(pixels: &TilePixels) -> Result<Bytes, png::EncodingError> {
    let color = match pixels.bands() {
        1 => png::ColorType::Grayscale,
        2 => png::ColorType::GrayscaleAlpha,
        3 => png::ColorType::Rgb,
        _ => png::ColorType::Rgba,
    };
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, pixels.size(), pixels.size());
    encoder.set_color(color);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(pixels.data())?;
    writer.finish()?;
    Ok(Bytes::from(out))
}

/// Encodes a tile of 1 to 4 bands as WebP, which holds only RGB and RGBA.
#[cfg(feature = "webp")]
fn encode_webp(pixels: &TilePixels, quality: Option<u8>) -> Bytes {
    let expanded: Vec<u8>;
    let (data, layout) = match pixels.bands() {
        1 => {
            expanded = pixels.data().iter().flat_map(|&v| [v, v, v]).collect();
            (expanded.as_slice(), webp::PixelLayout::Rgb)
        }
        2 => {
            expanded = pixels
                .data()
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[0], p[0], p[1]])
                .collect();
            (expanded.as_slice(), webp::PixelLayout::Rgba)
        }
        3 => (pixels.data(), webp::PixelLayout::Rgb),
        _ => (pixels.data(), webp::PixelLayout::Rgba),
    };
    let encoder = webp::Encoder::new(data, layout, pixels.size(), pixels.size());
    let webp = match quality {
        Some(quality) => encoder.encode(f32::from(quality.clamp(1, 100))),
        None => encoder.encode_lossless(),
    };
    Bytes::copy_from_slice(&webp)
}

/// Encodes a tile of 1 to 4 bands as JPEG, painting any alpha band over `background`.
#[cfg(feature = "jpeg")]
fn encode_jpeg(pixels: &TilePixels, quality: u8, background: [u8; 3]) -> image::ImageResult<Bytes> {
    use image::ExtendedColorType;
    use image::codecs::jpeg::JpegEncoder;

    let over = |value: u8, alpha: u8, under: u8| {
        let mixed = u16::from(value) * u16::from(alpha) + u16::from(under) * u16::from(255 - alpha);
        u8::try_from((mixed + 127) / 255).expect("a blend of two 8-bit values is 8-bit")
    };
    let painted: Vec<u8>;
    let (data, color) = match pixels.bands() {
        1 => (pixels.data(), ExtendedColorType::L8),
        2 => {
            painted = pixels
                .data()
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|p| background.map(|under| over(p[0], p[1], under)))
                .collect();
            (painted.as_slice(), ExtendedColorType::Rgb8)
        }
        3 => (pixels.data(), ExtendedColorType::Rgb8),
        _ => {
            painted = pixels
                .data()
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [0, 1, 2].map(|band| over(p[band], p[3], background[band])))
                .collect();
            (painted.as_slice(), ExtendedColorType::Rgb8)
        }
    };
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100)).encode(data, pixels.size(), pixels.size(), color)?;
    Ok(Bytes::from(out))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn tile(pixel: &[u8]) -> TilePixels {
        TilePixels::padded(pixel.repeat(16 * 16), [16, 16, pixel.len()], 16, "test").unwrap()
    }

    fn decode(bytes: &[u8]) -> image::DynamicImage {
        image::load_from_memory(bytes).unwrap()
    }

    #[test]
    fn rejects_band_counts_no_format_can_hold() {
        let pixels = TilePixels::padded(vec![0; 5], [1, 1, 5], 1, "test").unwrap();
        insta::assert_snapshot!(
            TileEncoding::Png.encode(&pixels).unwrap_err(),
            @"tile cannot be read as tiles: 5 samples per pixel cannot be encoded as Png"
        );
    }

    #[rstest]
    #[case::gray(&[200])]
    #[case::gray_alpha(&[200, 128])]
    #[case::rgb(&[10, 20, 30])]
    #[case::rgba(&[10, 20, 30, 0])]
    fn png_keeps_every_band(#[case] pixel: &[u8]) {
        let pixels = tile(pixel);
        let decoded = decode(&TileEncoding::Png.encode(&pixels).unwrap());
        assert_eq!(decoded.as_bytes(), pixels.data());
    }

    #[cfg(feature = "webp")]
    #[rstest]
    #[case::gray(&[200], [200, 200, 200, 255])]
    #[case::gray_alpha(&[200, 128], [200, 200, 200, 128])]
    #[case::rgb(&[10, 20, 30], [10, 20, 30, 255])]
    #[case::rgba(&[10, 20, 30, 128], [10, 20, 30, 128])]
    fn lossless_webp_keeps_every_pixel(#[case] pixel: &[u8], #[case] rgba: [u8; 4]) {
        let webp = TileEncoding::Webp { quality: None }.encode(&tile(pixel)).unwrap();
        assert!(decode(&webp).to_rgba8().pixels().all(|p| p.0 == rgba));
    }

    #[cfg(feature = "webp")]
    #[test]
    fn lossy_webp_keeps_alpha_exact() {
        let webp = TileEncoding::Webp { quality: Some(10) }
            .encode(&tile(&[10, 20, 30, 0]))
            .unwrap();
        assert!(decode(&webp).to_rgba8().pixels().all(|p| p.0[3] == 0));
    }

    #[cfg(feature = "jpeg")]
    #[rstest]
    #[case::transparent_gray(&[0, 0], [0, 0, 255])]
    #[case::transparent_rgb(&[200, 200, 200, 0], [0, 0, 255])]
    #[case::opaque_rgb(&[200, 200, 200, 255], [200, 200, 200])]
    #[case::half_transparent_rgb(&[255, 255, 255, 128], [128, 128, 255])]
    fn jpeg_paints_transparency_over_the_background(#[case] pixel: &[u8], #[case] rgb: [u8; 3]) {
        let jpeg = TileEncoding::Jpeg {
            quality: 100,
            background: [0, 0, 255],
        }
        .encode(&tile(pixel))
        .unwrap();
        let center = decode(&jpeg).to_rgb8().get_pixel(8, 8).0;
        assert!(
            center.iter().zip(rgb).all(|(&got, want)| got.abs_diff(want) <= 2),
            "{center:?} is not {rgb:?}"
        );
    }

    #[rstest]
    #[case::png(TileEncoding::Png, image::ImageFormat::Png)]
    #[cfg_attr(feature = "webp", case::webp(TileEncoding::Webp { quality: Some(80) }, image::ImageFormat::WebP))]
    #[cfg_attr(
        feature = "jpeg",
        case::jpeg(TileEncoding::Jpeg { quality: 80, background: [0; 3] }, image::ImageFormat::Jpeg)
    )]
    fn encodes_the_format_it_reports(#[case] encoding: TileEncoding, #[case] image_format: image::ImageFormat) {
        let bytes = encoding.encode(&tile(&[1, 2, 3])).unwrap();
        assert_eq!(image::guess_format(&bytes).unwrap(), image_format);
        assert_eq!(encoding.format().extension(), image_format.extensions_str()[0]);
    }
}

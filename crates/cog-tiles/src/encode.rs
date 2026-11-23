//! Re-encoding decoded 8-bit tiles as PNG, WebP or JPEG.

use bytes::Bytes;

use crate::{CogError, CogResult, TileFormat, TilePixels, UnsupportedReason};

/// The format and settings decoded tiles are encoded with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TileEncoding {
    /// Lossless PNG, fast to encode and large.
    #[default]
    Png,
    /// WebP, with lossless alpha.
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

    /// Encodes one decoded tile on the calling task, naming `location` in errors.
    ///
    /// # Errors
    /// Returns an error if `pixels` has more than four bands, or the encoder fails.
    pub fn encode(self, pixels: &TilePixels, location: &str) -> CogResult<Bytes> {
        let channels = Channels::of(pixels.bands()).ok_or_else(|| {
            CogError::unsupported(
                location,
                UnsupportedReason::EncodeSamples {
                    format: self.format(),
                    samples: pixels.bands(),
                },
            )
        })?;
        let backend_error = |e: Box<dyn std::error::Error + Send + Sync>| CogError::Backend {
            location: location.to_owned(),
            source: e,
        };
        match self {
            Self::Png => encode_png(pixels, channels).map_err(|e| backend_error(e.into())),
            #[cfg(feature = "webp")]
            Self::Webp { quality } => Ok(encode_webp(pixels, channels, quality)),
            #[cfg(feature = "jpeg")]
            Self::Jpeg { quality, background } => {
                encode_jpeg(pixels, channels, quality, background).map_err(|e| backend_error(e.into()))
            }
        }
    }
}

/// The channels of a decoded tile, named by its band count.
#[derive(Clone, Copy)]
enum Channels {
    /// One gray band.
    Gray,
    /// A gray band and an alpha band.
    GrayAlpha,
    /// Red, green and blue bands.
    Rgb,
    /// Red, green, blue and alpha bands.
    Rgba,
}

impl Channels {
    /// The channels `bands` bands hold, if any format can encode them.
    fn of(bands: usize) -> Option<Self> {
        match bands {
            1 => Some(Self::Gray),
            2 => Some(Self::GrayAlpha),
            3 => Some(Self::Rgb),
            4 => Some(Self::Rgba),
            _ => None,
        }
    }
}

/// Encodes a tile as PNG.
fn encode_png(pixels: &TilePixels, channels: Channels) -> Result<Bytes, png::EncodingError> {
    let color = match channels {
        Channels::Gray => png::ColorType::Grayscale,
        Channels::GrayAlpha => png::ColorType::GrayscaleAlpha,
        Channels::Rgb => png::ColorType::Rgb,
        Channels::Rgba => png::ColorType::Rgba,
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

/// Encodes a tile as WebP, widening gray to RGB because WebP holds only RGB and RGBA.
#[cfg(feature = "webp")]
fn encode_webp(pixels: &TilePixels, channels: Channels, quality: Option<u8>) -> Bytes {
    let expanded: Vec<u8>;
    let (data, layout) = match channels {
        Channels::Gray => {
            expanded = pixels.data().iter().flat_map(|&v| [v, v, v]).collect();
            (expanded.as_slice(), webp::PixelLayout::Rgb)
        }
        Channels::GrayAlpha => {
            expanded = pixels
                .data()
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[0], p[0], p[1]])
                .collect();
            (expanded.as_slice(), webp::PixelLayout::Rgba)
        }
        Channels::Rgb => (pixels.data(), webp::PixelLayout::Rgb),
        Channels::Rgba => (pixels.data(), webp::PixelLayout::Rgba),
    };
    let encoder = webp::Encoder::new(data, layout, pixels.size(), pixels.size());
    let webp = match quality {
        Some(quality) => encoder.encode(f32::from(quality.clamp(1, 100))),
        None => encoder.encode_lossless(),
    };
    Bytes::copy_from_slice(&webp)
}

/// Encodes a tile as JPEG, painting any alpha band over `background`.
#[cfg(feature = "jpeg")]
fn encode_jpeg(pixels: &TilePixels, channels: Channels, quality: u8, background: [u8; 3]) -> image::ImageResult<Bytes> {
    use image::ExtendedColorType;
    use image::codecs::jpeg::JpegEncoder;

    let over = |value: u8, alpha: u8, under: u8| {
        let mixed = u16::from(value) * u16::from(alpha) + u16::from(under) * u16::from(255 - alpha);
        u8::try_from((mixed + 127) / 255).expect("a blend of two 8-bit values is 8-bit")
    };
    let painted: Vec<u8>;
    let (data, color) = match channels {
        Channels::Gray => (pixels.data(), ExtendedColorType::L8),
        Channels::GrayAlpha => {
            painted = pixels
                .data()
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|p| background.map(|under| over(p[0], p[1], under)))
                .collect();
            (painted.as_slice(), ExtendedColorType::Rgb8)
        }
        Channels::Rgb => (pixels.data(), ExtendedColorType::Rgb8),
        Channels::Rgba => {
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
    #[cfg(feature = "webp")]
    use std::collections::BTreeSet;

    use super::*;

    fn tile(pixel: &[u8]) -> TilePixels {
        TilePixels::padded(pixel.repeat(16 * 16), [16, 16, pixel.len()], 16, "test").unwrap()
    }

    fn decode(bytes: &[u8]) -> image::DynamicImage {
        image::load_from_memory(bytes).unwrap()
    }

    #[cfg(feature = "webp")]
    fn distinct_rgba(bytes: &[u8]) -> BTreeSet<[u8; 4]> {
        decode(bytes).to_rgba8().pixels().map(|p| p.0).collect()
    }

    fn png_round_trip(pixel: &[u8]) {
        let pixels = tile(pixel);
        let decoded = decode(&TileEncoding::Png.encode(&pixels, "test").unwrap());
        assert_eq!(decoded.as_bytes(), pixels.data());
    }

    #[test]
    fn rejects_band_counts_no_format_can_hold() {
        let pixels = TilePixels::padded(vec![0; 5], [1, 1, 5], 1, "test").unwrap();
        insta::assert_snapshot!(
            TileEncoding::Png.encode(&pixels, "test.tif").unwrap_err(),
            @"test.tif cannot be read as tiles: 5 samples per pixel cannot be encoded as PNG"
        );
    }

    #[test]
    fn png_keeps_a_gray_band() {
        png_round_trip(&[200]);
    }

    #[test]
    fn png_keeps_gray_and_alpha_bands() {
        png_round_trip(&[200, 128]);
    }

    #[test]
    fn png_keeps_rgb_bands() {
        png_round_trip(&[10, 20, 30]);
    }

    #[test]
    fn png_keeps_rgba_bands() {
        png_round_trip(&[10, 20, 30, 0]);
    }

    #[cfg(feature = "webp")]
    #[test]
    fn lossless_webp_widens_gray_to_rgb() {
        let webp = TileEncoding::Webp { quality: None }
            .encode(&tile(&[200]), "test")
            .unwrap();
        insta::assert_debug_snapshot!(distinct_rgba(&webp), @r"
        {
            [
                200,
                200,
                200,
                255,
            ],
        }
        ");
    }

    #[cfg(feature = "webp")]
    #[test]
    fn lossless_webp_widens_gray_and_keeps_alpha() {
        let webp = TileEncoding::Webp { quality: None }
            .encode(&tile(&[200, 128]), "test")
            .unwrap();
        insta::assert_debug_snapshot!(distinct_rgba(&webp), @r"
        {
            [
                200,
                200,
                200,
                128,
            ],
        }
        ");
    }

    #[cfg(feature = "webp")]
    #[test]
    fn lossless_webp_keeps_rgb() {
        let webp = TileEncoding::Webp { quality: None }
            .encode(&tile(&[10, 20, 30]), "test")
            .unwrap();
        insta::assert_debug_snapshot!(distinct_rgba(&webp), @r"
        {
            [
                10,
                20,
                30,
                255,
            ],
        }
        ");
    }

    #[cfg(feature = "webp")]
    #[test]
    fn lossless_webp_keeps_rgba() {
        let webp = TileEncoding::Webp { quality: None }
            .encode(&tile(&[10, 20, 30, 128]), "test")
            .unwrap();
        insta::assert_debug_snapshot!(distinct_rgba(&webp), @r"
        {
            [
                10,
                20,
                30,
                128,
            ],
        }
        ");
    }

    #[cfg(feature = "webp")]
    #[test]
    fn lossy_webp_keeps_alpha_exact() {
        let webp = TileEncoding::Webp { quality: Some(10) }
            .encode(&tile(&[10, 20, 30, 0]), "test")
            .unwrap();
        let alphas: BTreeSet<u8> = distinct_rgba(&webp).iter().map(|p| p[3]).collect();
        insta::assert_debug_snapshot!(alphas, @r"
        {
            0,
        }
        ");
    }

    #[cfg(feature = "jpeg")]
    fn jpeg_center(pixel: &[u8]) -> [u8; 3] {
        let jpeg = TileEncoding::Jpeg {
            quality: 100,
            background: [0, 0, 255],
        }
        .encode(&tile(pixel), "test")
        .unwrap();
        decode(&jpeg).to_rgb8().get_pixel(8, 8).0
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn jpeg_paints_transparent_gray_as_the_background() {
        insta::assert_debug_snapshot!(jpeg_center(&[0, 0]), @"
        [
            0,
            0,
            254,
        ]
        ");
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn jpeg_paints_transparent_rgb_as_the_background() {
        insta::assert_debug_snapshot!(jpeg_center(&[200, 200, 200, 0]), @"
        [
            0,
            0,
            254,
        ]
        ");
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn jpeg_keeps_opaque_rgb() {
        insta::assert_debug_snapshot!(jpeg_center(&[200, 200, 200, 255]), @"
        [
            200,
            200,
            200,
        ]
        ");
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn jpeg_blends_half_transparent_rgb_with_the_background() {
        insta::assert_debug_snapshot!(jpeg_center(&[255, 255, 255, 128]), @"
        [
            128,
            127,
            254,
        ]
        ");
    }

    #[test]
    fn png_encodes_png() {
        let bytes = TileEncoding::Png.encode(&tile(&[1, 2, 3]), "test").unwrap();
        assert_eq!(image::guess_format(&bytes).unwrap(), image::ImageFormat::Png);
        assert_eq!(TileEncoding::Png.format(), TileFormat::Png);
    }

    #[cfg(feature = "webp")]
    #[test]
    fn webp_encodes_webp() {
        let encoding = TileEncoding::Webp { quality: Some(80) };
        let bytes = encoding.encode(&tile(&[1, 2, 3]), "test").unwrap();
        assert_eq!(image::guess_format(&bytes).unwrap(), image::ImageFormat::WebP);
        assert_eq!(encoding.format(), TileFormat::Webp);
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn jpeg_encodes_jpeg() {
        let encoding = TileEncoding::Jpeg {
            quality: 80,
            background: [0; 3],
        };
        let bytes = encoding.encode(&tile(&[1, 2, 3]), "test").unwrap();
        assert_eq!(image::guess_format(&bytes).unwrap(), image::ImageFormat::Jpeg);
        assert_eq!(encoding.format(), TileFormat::Jpeg);
    }
}

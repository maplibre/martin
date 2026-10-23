//! Re-encoding decoded 8-bit tiles as PNG.

use bytes::Bytes;

use crate::{CogError, CogResult, TilePixels, UnsupportedReason};

/// Encodes a decoded tile as PNG.
pub(crate) fn encode_png(pixels: &TilePixels, location: &str) -> CogResult<Bytes> {
    let color = match pixels.bands() {
        1 => png::ColorType::Grayscale,
        2 => png::ColorType::GrayscaleAlpha,
        3 => png::ColorType::Rgb,
        4 => png::ColorType::Rgba,
        samples => {
            return Err(CogError::unsupported(
                location,
                UnsupportedReason::PngSamples { samples },
            ));
        }
    };
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, pixels.size(), pixels.size());
    encoder.set_color(color);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let png_error = |e: png::EncodingError| CogError::Backend {
        location: location.to_owned(),
        source: Box::new(e),
    };
    let mut writer = encoder.write_header().map_err(png_error)?;
    writer.write_image_data(pixels.data()).map_err(png_error)?;
    writer.finish().map_err(png_error)?;
    Ok(Bytes::from(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_band_counts_png_cannot_hold() {
        let pixels = TilePixels::padded(vec![0; 5], [1, 1, 5], 1, "test").unwrap();
        encode_png(&pixels, "test").unwrap_err();
    }
}

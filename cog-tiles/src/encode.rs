//! Re-encoding decoded 8-bit tiles as PNG.

use bytes::Bytes;

use crate::{CogError, CogResult, UnsupportedReason};

/// Encodes `pixels` (row-major, `samples` interleaved bytes per pixel) as a square PNG.
///
/// A decoded tile smaller than `tile_size` is padded with transparent (or black) pixels.
pub(crate) fn encode_png(
    pixels: &[u8],
    [height, width, samples]: [usize; 3],
    tile_size: u32,
    location: &str,
) -> CogResult<Bytes> {
    let color = match samples {
        1 => png::ColorType::Grayscale,
        2 => png::ColorType::GrayscaleAlpha,
        3 => png::ColorType::Rgb,
        4 => png::ColorType::Rgba,
        _ => {
            return Err(CogError::unsupported(
                location,
                UnsupportedReason::PngSamples { samples },
            ));
        }
    };
    let size = tile_size as usize;
    if pixels.len() != height * width * samples || height > size || width > size {
        return Err(CogError::unsupported(
            location,
            UnsupportedReason::DecodedSize {
                width,
                height,
                samples,
                size,
            },
        ));
    }
    let padded;
    let pixels = if height == size && width == size {
        pixels
    } else {
        let mut buf = vec![0; size * size * samples];
        for (row, src) in pixels.chunks_exact(width * samples).enumerate() {
            let start = row * size * samples;
            buf[start..start + src.len()].copy_from_slice(src);
        }
        padded = buf;
        &padded
    };

    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, tile_size, tile_size);
    encoder.set_color(color);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let png_error = |e: png::EncodingError| CogError::Backend {
        location: location.to_owned(),
        source: Box::new(e),
    };
    let mut writer = encoder.write_header().map_err(png_error)?;
    writer.write_image_data(pixels).map_err(png_error)?;
    writer.finish().map_err(png_error)?;
    Ok(Bytes::from(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_partial_tiles() {
        let png = encode_png(&[255; 2 * 3], [1, 2, 3], 4, "test").unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!(info.width, 4);
        assert_eq!(info.height, 4);
        assert_eq!(&buf[..6], &[255; 6]);
        assert!(buf[6..].iter().all(|&v| v == 0));
    }

    #[test]
    fn rejects_unknown_layouts() {
        encode_png(&[0; 5], [1, 1, 5], 1, "test").unwrap_err();
        encode_png(&[0; 3], [1, 1, 1], 1, "test").unwrap_err();
    }
}

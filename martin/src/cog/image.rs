use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use martin_tile_utils::TileCoord;
use tiff::decoder::{Decoder, DecodingResult};

use super::CogError;
use crate::{MartinResult, TileData};

/// Image represents a single image in a COG file. A tiff file may contain many images.
/// This type contains several useful information and methods for taking tiles from the image.
#[derive(Clone, Debug)]
pub struct Image {
    /// The Number of Image file directory, generally abbreviated as IFD.
    /// An IFD contains information about the image, as well as pointers to the actual image data.
    pub image_file_directory: usize,
    /// Number of tiles in a row of this image
    pub across: u32,
    ///  Number of tiles in a column of this image
    pub down: u32,
}

impl Image {
    /// Retrieves a tile from the image, decodes it, and converts it to PNG format.
    ///
    /// # Arguments
    /// * `decoder` - A mutable reference to a TIFF decoder.
    /// * `xyz` - The tile coordinates (z, x, y).
    /// * [nodata](https://gdal.org/en/stable/drivers/raster/gtiff.html#nodata-value) - An optional nodata value. Pixels with this value will be made transparent.
    /// * `path` - The path to the TIFF file, used for error reporting.
    ///
    /// # Returns
    /// A `MartinResult` containing the tile data as a `Vec<u8>` (PNG bytes) or an error.
    #[allow(clippy::cast_sign_loss)]
    #[allow(clippy::cast_possible_truncation)]
    pub fn get_tile(
        &self,
        decoder: &mut Decoder<File>,
        xyz: TileCoord,
        nodata: Option<f64>,
        path: &Path,
    ) -> MartinResult<TileData> {
        decoder
            .seek_to_image(self.image_file_directory)
            .map_err(|e| {
                CogError::IfdSeekFailed(e, self.image_file_directory, path.to_path_buf())
            })?;

        let tile_idx;
        if let Some(idx) = self.get_tile_idx(xyz) {
            tile_idx = idx;
        } else {
            return Ok(Vec::new());
        }
        let decode_result = decoder.read_chunk(tile_idx).map_err(|e| {
            CogError::ReadChunkFailed(e, tile_idx, self.image_file_directory, path.to_path_buf())
        })?;
        let color_type = decoder
            .colortype()
            .map_err(|e| CogError::InvalidTiffFile(e, path.to_path_buf()))?;

        let (tile_width, tile_height) = decoder.chunk_dimensions();
        let (data_width, data_height) = decoder.chunk_data_dimensions(tile_idx);

        //FIXME: do more research on the not u8 case, is this the right way to do it?
        let png_file_bytes = match (decode_result, color_type) {
            (DecodingResult::U8(vec), tiff::ColorType::RGB(_)) => rgb_to_png(
                vec,
                (tile_width, tile_height),
                (data_width, data_height),
                3,
                nodata.map(|v| v as u8),
                path,
            ),
            (DecodingResult::U8(vec), tiff::ColorType::RGBA(_)) => rgb_to_png(
                vec,
                (tile_width, tile_height),
                (data_width, data_height),
                4,
                nodata.map(|v| v as u8),
                path,
            ),
            (_, _) => Err(CogError::NotSupportedColorTypeAndBitDepth(
                color_type,
                path.to_path_buf(),
            )),
            //todo do others in next PRs, a lot of discussion would be needed
        }?;
        Ok(png_file_bytes)
    }
    fn get_tile_idx(&self, xyz: TileCoord) -> Option<u32> {
        if xyz.y >= self.down || xyz.x >= self.across {
            return None;
        }

        let tile_idx = xyz.y * self.across + xyz.x;
        Some(tile_idx)
    }
}

/// Converts RGB/RGBA tile data to PNG format.
///
/// # Arguments
/// * `data` - Raw pixel data from TIFF decoder
/// * `tile_width`, `tile_height` - Expected tile dimensions
/// * `data_width`, `data_height` - Actual data dimensions
/// * `components_count` - Number of color components (3 for RGB, 4 for RGBA)
/// * `nodata` - Optional nodata value to make transparent
/// * `path` - File path for error reporting
///
/// # Returns
/// PNG-encoded tile data as bytes
fn rgb_to_png(
    data: Vec<u8>,
    (tile_width, tile_height): (u32, u32),
    (data_width, data_height): (u32, u32),
    components_count: u32,
    nodata: Option<u8>,
    path: &Path,
) -> Result<Vec<u8>, CogError> {
    let pixels = ensure_pixels_valid(
        data,
        (tile_width, tile_height),
        (data_width, data_height),
        components_count,
        nodata,
    );
    encode_rgba_to_png(tile_width, tile_height, &pixels, path)
}

/// Ensures pixel data is valid for PNG encoding by handling padding, alpha channel, and nodata values.
///
/// # Arguments
/// * `data` - Raw pixel data
/// * `tile_width`, `tile_height` - Target tile dimensions
/// * `data_width`, `data_height` - Source data dimensions
/// * `components_count` - Number of color components per pixel
/// * `nodata` - Optional value to treat as transparent
///
/// # Returns
/// RGBA pixel data ready for PNG encoding
fn ensure_pixels_valid(
    data: Vec<u8>,
    (tile_width, tile_height): (u32, u32),
    (data_width, data_height): (u32, u32),
    components_count: u32,
    nodata: Option<u8>,
) -> Vec<u8> {
    let is_padded = data_width != tile_width || data_height != tile_height;
    let need_add_alpha = components_count != 4;
    // 1. Check if the tile is padded, if so, we need to add padding part back
    //  The decoded might be smaller than the tile size as tiff crate always cut off the padding part
    //  So we need to add the padding part back if needed
    // 2. Check if we need to add alpha channel, if the components count is not 4(rgba => 4 components, rgb => 3 components), we need to add an alpha channel
    // 3. Check if nodata is provided, if so, we need to make the pixels with nodata value transparent
    //    See https://gdal.org/en/stable/drivers/raster/gtiff.html#nodata-value
    if nodata.is_some() || need_add_alpha || is_padded {
        let mut result_vec = vec![0; (tile_width * tile_height * 4) as usize];
        for row in 0..data_height {
            'outer: for col in 0..data_width {
                let idx_chunk = row * data_width * components_count + col * components_count;
                let idx_result = row * tile_width * 4 + col * 4;

                // Copy the components one by one
                for component_idx in 0..components_count {
                    // Before copying, check if this component == nodata. If so, do skip and it would be transparent.
                    // FIXME: Should we copy the RGB values anyway and just set alpha to 0? The visual result actually is the same (transparent), but the component values would differ. But it might be a little slower as we don't skip the copy
                    //      Source pixel: [4, 1, 2, 3]  nodata: Some(1)
                    //      Do skip:
                    //      result pixel: [4, 0, 0, 0]
                    //      Do not skip:
                    //      result pixel: [4, 1, 2, 0]
                    //      So the visual result is the same, but the component values are different.

                    if nodata.eq(&Some(data[(idx_chunk + component_idx) as usize])) {
                        continue 'outer;
                    }
                    // Copy this component to the result vector
                    result_vec[(idx_result + component_idx) as usize] =
                        data[(idx_chunk + component_idx) as usize];
                }
                // If an alpha channel needs to be added, set it to 255 (opaque)
                if need_add_alpha {
                    let alpha_idx = (idx_result + 3) as usize;
                    result_vec[alpha_idx] = 255;
                }
            }
        }
        result_vec
    } else {
        data
    }
}

/// Encodes RGBA pixel data to PNG format.
///
/// # Arguments
/// * `tile_width`, `tile_height` - Image dimensions
/// * `pixels` - RGBA pixel data
/// * `path` - File path for error reporting
///
/// # Returns
/// PNG-encoded image data as bytes
fn encode_rgba_to_png(
    tile_width: u32,
    tile_height: u32,
    pixels: &[u8],
    path: &Path,
) -> Result<Vec<u8>, CogError> {
    let mut result_file_buffer = Vec::new();
    {
        let mut encoder = png::Encoder::new(
            BufWriter::new(&mut result_file_buffer),
            tile_width,
            tile_height,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| CogError::WritePngHeaderFailed(path.to_path_buf(), e))?;
        writer
            .write_image_data(pixels)
            .map_err(|e| CogError::WriteToPngFailed(path.to_path_buf(), e))?;
    }
    Ok(result_file_buffer)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use martin_tile_utils::TileCoord;
    use rstest::rstest;

    use crate::cog::image::Image;

    #[test]
    fn can_calc_tile_idx() {
        let image = Image {
            image_file_directory: 0,
            across: 3,
            down: 3,
        };
        assert_eq!(Some(0), image.get_tile_idx(TileCoord { z: 0, x: 0, y: 0 }));
        assert_eq!(Some(8), image.get_tile_idx(TileCoord { z: 0, x: 2, y: 2 }));
        assert_eq!(None, image.get_tile_idx(TileCoord { z: 0, x: 3, y: 0 }));
        assert_eq!(None, image.get_tile_idx(TileCoord { z: 0, x: 1, y: 9 }));
    }
    #[rstest]
    // the right half should be transparent
    #[case(
        "../tests/fixtures/cog/expected/right_padded.png",
        (0,0,0,None),None,(128,256),(256,256)
    )]
    // the down half should be transparent
    #[case(
        "../tests/fixtures/cog/expected/down_padded.png",
        (0,0,0,None),None,(256,128),(256,256)
    )]
    // the up half should be half transparent and down half should be transparent
    #[case(
        "../tests/fixtures/cog/expected/down_padded_with_alpha.png",
        (0,0,0,Some(128)),None,(256,128),(256,256)
    )]
    // the left half should be half transparent and the right half should be transparent
    #[case(
        "../tests/fixtures/cog/expected/right_padded_with_alpha.png",
        (0,0,0,Some(128)),None,(128,256),(256,256)
    )]
    // should be all half transparent
    #[case(
        "../tests/fixtures/cog/expected/not_padded.png",
        (0,0,0,Some(128)),None,(256,256),(256,256)
    )]
    // all padded and with a no_data whose value is 128, and all the component is 128
    // so that should be all transparent
    #[case(
        "../tests/fixtures/cog/expected/all_transparent.png",
        (128,128,128,Some(128)),Some(128),(128,128),(256,256)
    )]
    fn test_padded_cases(
        #[case] expected_file_path: &str,
        #[case] components: (u8, u8, u8, Option<u8>),
        #[case] no_value: Option<u8>,
        #[case] (data_width, data_height): (u32, u32),
        #[case] (tile_width, tile_height): (u32, u32),
    ) {
        let mut pixels = Vec::new();
        for _ in 0..(data_width * data_height) {
            pixels.push(components.0);
            pixels.push(components.1);
            pixels.push(components.2);
            if let Some(alpha) = components.3 {
                pixels.push(alpha);
            }
        }
        let components_count = if components.3.is_some() { 4 } else { 3 };
        let png_bytes = super::rgb_to_png(
            pixels,
            (tile_width, tile_height),
            (data_width, data_height),
            components_count,
            no_value,
            &PathBuf::from("not_exist.tif"),
        )
        .unwrap();
        let expected = std::fs::read(expected_file_path).unwrap();
        assert_eq!(png_bytes, expected);
    }
}

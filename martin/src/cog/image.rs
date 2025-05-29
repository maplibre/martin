use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use martin_tile_utils::TileCoord;
use tiff::decoder::{Decoder, DecodingResult};

use super::CogError;
use crate::{MartinResult, TileData};

/// Image represents a single image in a COG file. A tiff file may contain many images.
/// This type contains several useful information and method for taking tiles from the image.
#[derive(Clone, Debug)]
pub struct Image {
    /// IFD(Image file directory) number.
    /// An IFD contains information about the image, as well as pointers to the actual image data.
    pub ifd: usize,
    /// How many tiles in a row of this image
    pub across: u32,
    // How many tiles in a column of this image
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
            .seek_to_image(self.ifd)
            .map_err(|e| CogError::IfdSeekFailed(e, self.ifd, path.to_path_buf()))?;

        let tile_idx;
        if let Some(idx) = self.get_tile_idx(xyz) {
            tile_idx = idx;
        } else {
            return Ok(Vec::new());
        }
        let decode_result = decoder
            .read_chunk(tile_idx)
            .map_err(|e| CogError::ReadChunkFailed(e, tile_idx, self.ifd, path.to_path_buf()))?;
        let color_type = decoder
            .colortype()
            .map_err(|e| CogError::InvalidTiffFile(e, path.to_path_buf()))?;

        let (tile_width, tile_height) = decoder.chunk_dimensions();
        let (data_width, data_height) = decoder.chunk_data_dimensions(tile_idx);

        //do more research on the not u8 case, is this the right way to do it?
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
            // do others in next PRs, a lot of discussion would be needed
        }?;
        Ok(png_file_bytes)
    }
    fn get_tile_idx(&self, xyz: TileCoord) -> Option<u32> {
        let across = self.across;
        let down = self.down;
        if xyz.y >= down || xyz.x >= across {
            return None;
        }

        let tile_idx = xyz.y * across + xyz.x;
        if tile_idx >= across * down {
            return None;
        }
        Some(tile_idx)
    }
}

fn rgb_to_png(
    vec: Vec<u8>,
    (tile_width, tile_height): (u32, u32),
    (data_width, data_height): (u32, u32),
    chunk_components_count: u32,
    nodata: Option<u8>,
    path: &Path,
) -> Result<Vec<u8>, CogError> {
    let is_padded = data_width != tile_width || data_height != tile_height;
    let need_add_alpha = chunk_components_count != 4;

    let pixels = if nodata.is_some() || need_add_alpha || is_padded {
        let mut result_vec = vec![0; (tile_width * tile_height * 4) as usize];
        for row in 0..data_height {
            'outer: for col in 0..data_width {
                let idx_chunk =
                    row * data_width * chunk_components_count + col * chunk_components_count;
                let idx_result = row * tile_width * 4 + col * 4;
                for component_idx in 0..chunk_components_count {
                    if nodata.eq(&Some(vec[(idx_chunk + component_idx) as usize])) {
                        //This pixel is nodata, just make it transparent and skip it then
                        let alpha_idx = (idx_result + 3) as usize;
                        result_vec[alpha_idx] = 0;
                        continue 'outer;
                    }
                    result_vec[(idx_result + component_idx) as usize] =
                        vec[(idx_chunk + component_idx) as usize];
                }
                if need_add_alpha {
                    let alpha_idx = (idx_result + 3) as usize;
                    result_vec[alpha_idx] = 255;
                }
            }
        }
        result_vec
    } else {
        vec
    };
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
            .write_image_data(&pixels)
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
            ifd: 0,
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

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use image::{ImageBuffer, Rgba};
use martin_tile_utils::{TileCoord, TileData};
use tiff::ColorType;
use tiff::decoder::{Decoder, DecodingResult};

use super::CogError;
use crate::tiles::MartinCoreResult;

/// Image represents a single image in a COG file. A tiff file may contain many images.
/// This struct contains information and methods for taking tiles from the image.
#[derive(Clone, Debug)]
pub struct Image {
    /// The Image File Directory index represents IDF entry with the image pointers to the actual image data.
    ifd_index: usize,
    /// The extent of the image in model units, represented as [`min_x`, `min_y`, `max_x`, `max_y`].
    extent: [f64; 4],
    /// The origin of the image in model units.
    origin: [f64; 3],
    /// Number of tiles in a row of this image
    tiles_across: u32,
    /// Number of tiles in a column of this image
    tiles_down: u32,
    /// Tile size in pixels
    tile_size: (u32, u32),
    /// Resolution of the image in model units per pixel
    resolution: (f64, f64),
}

impl Image {
    pub fn new(
        ifd_index: usize,
        extent: [f64; 4],
        origin: [f64; 3],
        tiles_across: u32,
        tiles_down: u32,
        tile_size: (u32, u32),
        resolution: (f64, f64),
    ) -> Self {
        Self {
            ifd_index,
            extent,
            origin,
            tiles_across,
            tiles_down,
            tile_size,
            resolution,
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn get_tile_webmercator(
        &self,
        decoder: &mut Decoder<File>,
        xyz: TileCoord,
        nodata: Option<f64>,
        path: &Path,
    ) -> Result<TileData, CogError> {
        let bbox = martin_tile_utils::xyz_to_bbox_webmercator(xyz.z, xyz.x, xyz.y, xyz.x, xyz.y);
        #[allow(clippy::cast_sign_loss)]
        let nodata_u8 = nodata.map(|v| v as u8);
        let bytes = self.clip(decoder, bbox, 256, nodata_u8, path)?;
        Ok(bytes)
    }

    /// Clips the image to the specified bounding box and returns the PNG data.
    #[allow(clippy::cast_sign_loss)]
    #[allow(clippy::cast_possible_truncation)]
    fn clip(
        &self,
        decoder: &mut Decoder<File>,
        bbox: [f64; 4],
        output_size: u32,
        nodata: Option<u8>,
        path: &Path,
    ) -> Result<TileData, CogError> {
        decoder
            .seek_to_image(self.ifd_index())
            .map_err(|e| CogError::IfdSeekFailed(e, self.ifd_index(), path.to_path_buf()))?;

        let target_w = ((bbox[2] - bbox[0]) / self.resolution.0).round() as u32;
        let target_h = ((bbox[3] - bbox[1]) / self.resolution.1.abs()).round() as u32;
        let mut target = vec![0; (target_w * target_h * 4) as usize];

        // draw each tile on the target
        let intersected_tiles = tiles_intersected(
            self.tile_size,
            self.resolution,
            self.extent,
            (self.tiles_across, self.tiles_down),
            bbox,
        );
        for (col, row) in intersected_tiles {
            let Some(idx) = self.get_tile_index(TileCoord {
                z: 0, // actually this z is not used, so we can use 0 here
                x: col,
                y: row,
            }) else {
                continue;
            };

            let origin_x = self.origin[0];
            let origin_y = self.origin[1];
            let tile_min_x = origin_x + f64::from(col * self.tile_size.0) * self.resolution.0;
            let tile_max_y = origin_y - f64::from(row * self.tile_size.1) * self.resolution.1.abs();

            let geo_offset_x = tile_min_x - bbox[0];
            let geo_offset_y = bbox[3] - tile_max_y; // Use window's max Y

            let offset_x = (geo_offset_x / self.resolution.0).round() as i64;
            let offset_y = (geo_offset_y / self.resolution.1.abs()).round() as i64;

            let color_type = decoder
                .colortype()
                .map_err(|e| CogError::InvalidTiffFile(e, path.to_path_buf()))?; //FIXME: maybe make color_type as prop of Image Struct?
            let components_count = match color_type {
                ColorType::RGB(_) => 3,
                ColorType::RGBA(_) => 4,
                ct => {
                    return Err(CogError::NotSupportedColorTypeAndBitDepth(
                        ct,
                        path.to_path_buf(),
                    ))?;
                }
            };

            let (tile_w, tile_h) = decoder.chunk_data_dimensions(idx);
            let tile_data = decoder.read_chunk(idx).map_err(|e| {
                CogError::ReadChunkFailed(e, idx, self.ifd_index(), path.to_path_buf())
            })?;
            match (tile_data, color_type) {
                (DecodingResult::U8(vec), ColorType::RGB(_) | ColorType::RGBA(_)) => draw_tile(
                    &vec,
                    components_count,
                    nodata,
                    (tile_w, tile_h),
                    (target_w, target_h),
                    (offset_x, offset_y),
                    &mut target,
                ),
                (_, _) => {
                    return Err(CogError::NotSupportedColorTypeAndBitDepth(
                        color_type,
                        path.to_path_buf(),
                    ));
                } //todo do others in next PRs, a lot of discussion would be needed
            }
        }

        let result_image: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_raw(target_w, target_h, target).ok_or_else(|| {
                CogError::ImageBufferCreationFailed(
                    path.to_path_buf(),
                    format!("Failed to create image buffer with dimensions {target_w}x{target_h}"),
                )
            })?;
        let resized = image::imageops::resize(
            &result_image,
            output_size,
            output_size,
            image::imageops::FilterType::Nearest, //FIXME should make this configurable
        );
        let png = encode_rgba_as_png(output_size, output_size, resized.as_raw(), path)?;
        Ok(png)
    }

    /// Retrieves a tile from the image, decodes it, and converts it to PNG format.
    #[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    pub fn get_tile(
        &self,
        decoder: &mut Decoder<File>,
        xyz: TileCoord,
        nodata: Option<f64>,
        path: &Path,
    ) -> MartinCoreResult<TileData> {
        decoder
            .seek_to_image(self.ifd_index())
            .map_err(|e| CogError::IfdSeekFailed(e, self.ifd_index(), path.to_path_buf()))?;

        let tile_idx;
        if let Some(idx) = self.get_tile_index(xyz) {
            tile_idx = idx;
        } else {
            return Ok(Vec::new());
        }
        let decode_result = decoder.read_chunk(tile_idx).map_err(|e| {
            CogError::ReadChunkFailed(e, tile_idx, self.ifd_index(), path.to_path_buf())
        })?;
        let color_type = decoder
            .colortype()
            .map_err(|e| CogError::InvalidTiffFile(e, path.to_path_buf()))?;

        let (tile_width, tile_height) = decoder.chunk_dimensions();
        let (data_width, data_height) = decoder.chunk_data_dimensions(tile_idx);

        // FIXME: do more research on the not u8 case, is this the right way to do it?
        let png_file_bytes = match (decode_result, color_type) {
            (DecodingResult::U8(vec), ColorType::RGB(_)) => rgb_to_png(
                vec,
                (tile_width, tile_height),
                (data_width, data_height),
                3,
                nodata.map(|v| v as u8),
                path,
            ),
            (DecodingResult::U8(vec), ColorType::RGBA(_)) => rgb_to_png(
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
            // todo: do others in next PRs, a lot of discussion would be needed
        }?;
        Ok(png_file_bytes)
    }

    /// Returns the Image File Directory index for this image.
    pub fn ifd_index(&self) -> usize {
        self.ifd_index
    }

    pub fn resolution(&self) -> (f64, f64) {
        self.resolution
    }

    pub fn tile_size(&self) -> (u32, u32) {
        self.tile_size
    }

    fn get_tile_index(&self, xyz: TileCoord) -> Option<u32> {
        if xyz.y >= self.tiles_down || xyz.x >= self.tiles_across {
            return None;
        }

        Some(xyz.y * self.tiles_across + xyz.x)
    }
}

/// Calculates the tiles that intersect with the given window.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn tiles_intersected(
    tile_size: (u32, u32),
    resolution: (f64, f64),
    extent: [f64; 4],
    (tiles_across, tiles_down): (u32, u32),
    window: [f64; 4],
) -> Vec<(u32, u32)> {
    let epsilon = 1e-6;

    let tile_span_x = f64::from(tile_size.0) * resolution.0;
    // resolution[1] is typically negative, use its absolute value for span calculation
    let tile_span_y = f64::from(tile_size.1) * resolution.1.abs();

    let tile_matrix_min_x = extent[0];
    // Use max Y from extent as the top edge for row calculation
    let tile_matrix_max_y = extent[3];

    let matrix_width = tiles_across;
    let matrix_height = tiles_down;

    // Calculate tile index ranges based on the provided formula
    let tile_min_col_f = ((window[0] - tile_matrix_min_x) / tile_span_x + epsilon).floor();
    let tile_max_col_f = ((window[2] - tile_matrix_min_x) / tile_span_x - epsilon).floor();
    let tile_min_row_f = ((tile_matrix_max_y - window[3]) / tile_span_y + epsilon).floor();
    let tile_max_row_f = ((tile_matrix_max_y - window[1]) / tile_span_y - epsilon).floor();

    // Convert to integer type for clamping and iteration
    let mut tile_min_col = tile_min_col_f as i64;
    let mut tile_max_col = tile_max_col_f as i64;
    let mut tile_min_row = tile_min_row_f as i64;
    let mut tile_max_row = tile_max_row_f as i64;

    // Clamp minimum values to 0
    if tile_min_col < 0 {
        tile_min_col = 0;
    }
    if tile_min_row < 0 {
        tile_min_row = 0;
    }

    // Clamp maximum values to matrix dimensions - 1
    let matrix_width_i64 = i64::from(matrix_width);
    let matrix_height_i64 = i64::from(matrix_height);

    if tile_max_col >= matrix_width_i64 {
        tile_max_col = matrix_width_i64 - 1;
    }
    if tile_max_row >= matrix_height_i64 {
        tile_max_row = matrix_height_i64 - 1;
    }

    // If the calculated range is invalid (max < min), return empty vector
    if tile_max_col < tile_min_col || tile_max_row < tile_min_row {
        return Vec::new();
    }

    // Convert to u32 for the final result type
    let tile_min_col = tile_min_col as u32;
    let tile_max_col = tile_max_col as u32;
    let tile_min_row = tile_min_row as u32;
    let tile_max_row = tile_max_row as u32;

    let mut covered_tiles = Vec::new();
    // Iterate through the valid tile range and collect the indexes
    for row in tile_min_row..=tile_max_row {
        for col in tile_min_col..=tile_max_col {
            // Double check bounds (should be guaranteed by clamping, but safe)
            if col < matrix_width && row < matrix_height {
                covered_tiles.push((col, row));
            }
        }
    }

    covered_tiles
}

/// Converts RGB/RGBA tile data to PNG format.
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
    encode_rgba_as_png(tile_width, tile_height, &pixels, path)
}

/// Ensures pixel data is valid for PNG encoding by handling padding, alpha channel, and nodata values.
fn ensure_pixels_valid(
    data: Vec<u8>,
    (tile_width, tile_height): (u32, u32),
    (data_width, data_height): (u32, u32),
    components_count: u32,
    nodata: Option<u8>,
) -> Vec<u8> {
    let is_padded = data_width != tile_width || data_height != tile_height;
    let add_alpha = components_count == 3;
    // 1. Check if the tile is padded. If so, we need to add padding part back.
    //    The decoded value might be smaller than the tile size.
    //    TIFF crate always cut off the padding part, so we would need to add the padding part back.
    // 2. If the components count is 3 (RGB), we need to add the alpha channel to convert it to RGBA.
    // 3. Check if nodata is provided. We need to make the pixels with nodata value transparent
    //    See https://gdal.org/en/stable/drivers/raster/gtiff.html#nodata-value
    if nodata.is_some() || add_alpha || is_padded {
        let mut result_vec = vec![0; (tile_width * tile_height * 4) as usize];
        draw_tile(
            &data,
            components_count,
            nodata,
            (data_width, data_height),
            (tile_width, tile_height),
            (0, 0),
            &mut result_vec,
        );
        result_vec
    } else {
        data
    }
}

/// Encodes RGBA pixel data to PNG format.
fn encode_rgba_as_png(
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

/// Cover a tile(rgb/rgba) on a rgba buffer
#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_sign_loss)]
fn draw_tile(
    data: &[u8],
    components_count: u32,
    nodata: Option<u8>,
    (data_width, data_height): (u32, u32),
    (target_width, target_height): (u32, u32),
    (offset_x, offset_y): (i64, i64),
    target: &mut [u8],
) {
    let add_alpha = components_count != 4;
    for row in 0..data_height {
        'outer: for col in 0..data_width {
            let idx_chunk = row * data_width * components_count + col * components_count;
            let target_row = i64::from(row) + offset_y;
            let target_col = i64::from(col) + offset_x;
            if target_row < 0
                || target_col < 0
                || target_row >= i64::from(target_height)
                || target_col >= i64::from(target_width)
            {
                continue 'outer;
            }
            let target_row = target_row as usize;
            let target_col = target_col as usize;
            let idx_result = target_row * target_width as usize * 4 + target_col * 4;
            for component_idx in 0..components_count {
                let value = data[(idx_chunk + component_idx) as usize];
                if let Some(v) = nodata
                    && value == v
                {
                    continue 'outer;
                }
                // Copy this component to the result vector
                target[idx_result + component_idx as usize] = value;
            }
            if add_alpha {
                target[idx_result + 3_usize] = 255; // opaque
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use martin_tile_utils::{TileCoord, xyz_to_bbox_webmercator};
    use rstest::rstest;

    use crate::tiles::cog::image::Image;

    #[test]
    fn can_calc_tile_idx() {
        let image = Image {
            ifd_index: 0,
            origin: [0.0, 0.0, 0.0],
            extent: [0.0, 0.0, 0.0, 0.0],
            tiles_across: 3,
            tiles_down: 3,
            resolution: (1.0, 1.0),
            tile_size: (256, 256),
        };
        assert_eq!(
            Some(0),
            image.get_tile_index(TileCoord { z: 0, x: 0, y: 0 })
        );
        assert_eq!(
            Some(8),
            image.get_tile_index(TileCoord { z: 0, x: 2, y: 2 })
        );
        assert_eq!(None, image.get_tile_index(TileCoord { z: 0, x: 3, y: 0 }));
        assert_eq!(None, image.get_tile_index(TileCoord { z: 0, x: 1, y: 9 }));
    }
    #[rstest]
    // the right half should be transparent
    #[case(
        "right_padded.png",
        (0, 0, 0, None), None, (128, 256), (256, 256)
    )]
    // the down half should be transparent
    #[case(
        "down_padded.png",
        (0, 0, 0, None), None, (256, 128), (256, 256)
    )]
    // the up half should be half-transparent and down half should be transparent
    #[case(
        "down_padded_with_alpha.png",
        (0, 0, 0, Some(128)), None, (256, 128), (256, 256)
    )]
    // the left half should be half-transparent and the right half should be transparent
    #[case(
        "right_padded_with_alpha.png",
        (0, 0, 0, Some(128)), None, (128, 256), (256, 256)
    )]
    // should be all half transparent
    #[case(
        "not_padded.png",
        (0, 0, 0, Some(128)), None, (256, 256), (256, 256)
    )]
    // all padded and with a no_data whose value is 128, and all the component is 128
    // so that should be all transparent
    #[case(
        "all_transparent.png",
        (128, 128, 128, Some(128)), Some(128), (128, 128), (256, 256)
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
            Path::new("not_exist.tif"),
        )
        .unwrap();

        insta::assert_binary_snapshot!(expected_file_path, png_bytes);
    }

    // test bbox which aligned with tile boundary
    // these are edge cases need to be ensure
    #[rstest]
    #[case(0, 0, 0, 0, 0)]
    #[case(1, 0, 0, 0, 0)]
    #[case(1, 0, 0, 1, 1)]
    #[case(2, 0, 0, 0, 0)]
    #[case(2, 3, 3, 3, 3)]
    #[case(2, 1, 1, 2, 2)]
    #[case(2, 0, 0, 3, 3)]
    #[case(3, 4, 5, 6, 7)]
    #[case(4, 0, 0, 7, 0)]
    #[case(4, 7, 7, 7, 12)]
    #[case(1, 1, 1, 1, 1)]
    fn can_get_intersected_tiles_index(
        #[case] zoom: u8,
        #[case] min_x: u32,
        #[case] min_y: u32,
        #[case] max_x: u32,
        #[case] max_y: u32,
    ) {
        let tile_size = (256, 256);
        let extent = [
            -20_037_508.342_789_2,
            -20_037_508.342_789_2,
            20_037_508.342_789_2,
            20_037_508.342_789_2,
        ];
        let across = 2u32.pow(u32::from(zoom));
        let down = 2u32.pow(u32::from(zoom));

        let resolution = (
            (20_037_508.342_789_2 * 2.0) / (f64::from(across) * f64::from(tile_size.0)),
            -(20_037_508.342_789_2 * 2.0) / (f64::from(down) * f64::from(tile_size.1)),
        );

        let bbox = xyz_to_bbox_webmercator(zoom, min_x, min_y, max_x, max_y);

        let actual = super::tiles_intersected(tile_size, resolution, extent, (across, down), bbox);
        assert_eq!(
            actual.len() as usize,
            (max_x - min_x + 1) as usize * (max_y - min_y + 1) as usize
        );
        for row in min_y..=max_y {
            for col in min_x..=max_x {
                assert!(
                    actual.contains(&(col, row)),
                    "Tile ({col}, {row}) not found in the result"
                );
            }
        }
    }

    // test bbox which not aligned with tile boundary
    #[rstest]
    #[case(0, [-20_037_508.342_789_2 - 1000.0,
            -20_037_508.342_789_2 - 1000.0,
            20_037_508.342_789_2 + 1000.0,
            20_037_508.342_789_2 + 1000.0], (0,0,0,0))] // bigger than extent at aoom 0, should be [0,0,0,0]
    #[case(0, [-20_037_508.342_789_2 + 1000.0,
            -20_037_508.342_789_2 + 1000.0,
            20_037_508.342_789_2 - 1000.0,
            20_037_508.342_789_2 - 1000.0], (0,0,0,0))] // smaller than extent at aoom 0, should be [0,0,0,0]
    #[case(1, [-2000.0,1000.0,-1000.0,2000.0] ,(0,0,0,0))]
    #[case(1, [1000.0,-2000.0,2000.0,-1000.0] ,(1,1,1,1))]
    #[case(1, [-1000.0,
            -1000.0,1000.0,1000.0], (0,0,1,1))]
    fn tiles_intersected_with_bbox(
        #[case] zoom: u8,
        #[case] bbox: [f64; 4],
        #[case] expected: (u32, u32, u32, u32),
    ) {
        let tile_size = (256, 256);
        let extent = [
            -20_037_508.342_789_2,
            -20_037_508.342_789_2,
            20_037_508.342_789_2,
            20_037_508.342_789_2,
        ];
        let across = 2u32.pow(u32::from(zoom));
        let down = 2u32.pow(u32::from(zoom));
        let resolution = (
            (20_037_508.342_789_2 * 2.0) / (f64::from(across) * f64::from(tile_size.0)),
            -(20_037_508.342_789_2 * 2.0) / (f64::from(down) * f64::from(tile_size.1)),
        );

        let actual = super::tiles_intersected(tile_size, resolution, extent, (across, down), bbox);

        let (min_x, min_y, max_x, max_y) = expected;
        let expected_count = (max_x - min_x + 1) as usize * (max_y - min_y + 1) as usize;
        assert_eq!(actual.len(), expected_count, "Unexpected tile count");

        for row in min_y..=max_y {
            for col in min_x..=max_x {
                assert!(
                    actual.contains(&(col, row)),
                    "Tile ({col}, {row}) not found"
                );
            }
        }
    }
}

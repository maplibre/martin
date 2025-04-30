use std::collections::HashMap;
use std::fmt::Debug;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::vec;

use image::{ImageBuffer, Rgba};

use async_trait::async_trait;
use log::warn;
use martin_tile_utils::{Format, TileCoord, TileInfo};
use regex::Regex;
use serde::Serialize;
use tiff::TiffResult;
use tiff::decoder::{ChunkType, Decoder, DecodingResult};
use tiff::tags::Tag::{self, GdalNodata};
use tilejson::{TileJSON, tilejson};

use super::CogError;
use crate::file_config::{FileError, FileResult};
use crate::{MartinResult, Source, TileData, UrlQuery, utils};

// about the model space of tiff image.
// pixel scale, tie points and transformations
// todo use struct instead of tuple maybe
type ModelInfo = (Option<Vec<f64>>, Option<Vec<f64>>, Option<Vec<f64>>);

#[derive(Clone, Debug, Serialize)]
struct Meta {
    min_zoom: u8,
    max_zoom: u8,
    google_zoom: Option<(u8, u8)>,
    origin: [f64; 3],
    extent: [f64; 4],
    zoom_and_resolutions: HashMap<u8, [f64; 3]>,
    zoom_and_ifd: HashMap<u8, usize>,
    zoom_and_tile_across_down: HashMap<u8, (u32, u32)>,
    nodata: Option<f64>,
    tile_size: (u32, u32),
}

#[derive(Clone, Debug)]
pub struct CogSource {
    id: String,
    path: PathBuf,
    meta: Meta,
    tilejson: TileJSON,
    tileinfo: TileInfo,
    force_google: bool,
}

impl CogSource {
    pub fn new(id: String, path: PathBuf, force_google: bool) -> FileResult<Self> {
        let tileinfo = TileInfo::new(Format::Png, martin_tile_utils::Encoding::Uncompressed);
        let meta = get_meta(&path)?;

        let tilejson: TileJSON = meta_to_tilejson(&meta);
        let mut google_compatible = false;
        if force_google == true && meta.google_zoom.is_some() {
            google_compatible = true;
        }
        Ok(CogSource {
            id,
            path,
            meta,
            tilejson,
            tileinfo,
            force_google: google_compatible,
        })
    }

    pub fn sub_region(
        &self,
        decoder: &mut Decoder<File>,
        zoom: u8,
        window: [f64; 4],
        output_size: u32,
    ) -> MartinResult<TileData> {
        let ifd = self.meta.zoom_and_ifd.get(&zoom).unwrap();

        let resolution = self.meta.zoom_and_resolutions.get(&zoom).unwrap();
        let res_x = resolution[0];
        let res_y = resolution[1].abs();
        let window_width_pixel = ((window[2] - window[0]) / res_x).round() as u32;
        let window_height_pixel = ((window[3] - window[1]) / res_y).round() as u32;

        let cog_origin = self.meta.origin;
        let cog_extent = self.meta.extent;
        let cog_tile_size = self.meta.tile_size;
        let across_down = self.meta.zoom_and_tile_across_down.get(&zoom).unwrap();
        let no_data = self.meta.nodata;

        let tile_indexes: Vec<(u32, u32)> =
            get_covered_tile_indexes(window, cog_extent, *across_down, cog_tile_size, resolution);

        let mut output_image: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::new(window_width_pixel, window_height_pixel);

        decoder.seek_to_image(*ifd);
        for (col, row) in tile_indexes {
            let tile_idx = get_tile_idx(
                TileCoord {
                    z: zoom,
                    x: col,
                    y: row,
                },
                across_down.0,
                across_down.1,
            )
            .unwrap();
            let origin_x = cog_origin[0];
            let origin_y = cog_origin[1];
            let tile_min_x = origin_x + f64::from(col * cog_tile_size.0) * res_x;
            let tile_max_y = origin_y - f64::from(row * cog_tile_size.1) * res_y;

            let offset_x_geo = tile_min_x - window[0];
            let offset_y_geo = window[3] - tile_max_y; // Use window's max Y

            let offset_x_pixel = (offset_x_geo / res_x).round() as i64;
            let offset_y_pixel = (offset_y_geo / res_y).round() as i64;

            let (data_width, data_height) = decoder.chunk_data_dimensions(tile_idx);
            let decoded_result = decoder.read_chunk(tile_idx).unwrap();
            let color_type = decoder.colortype().unwrap();
            for y_tile in 0..data_height {
                for x_tile in 0..data_width {
                    let target_x = offset_x_pixel + i64::from(x_tile);
                    let target_y = offset_y_pixel + i64::from(y_tile);

                    if target_x < 0
                        || target_y < 0
                        || target_x >= window_width_pixel as i64
                        || target_y >= window_height_pixel as i64
                    {
                        continue;
                    }

                    match (color_type, &decoded_result) {
                        (tiff::ColorType::RGB(_), DecodingResult::U8(data)) => {
                            let idx = (y_tile * data_width + x_tile) * 3;
                            let r = data[idx as usize];
                            let g = data[idx as usize + 1];
                            let b = data[idx as usize + 2];
                            if let Some(nodata) = no_data {
                                if r == nodata as u8 || g == nodata as u8 || b == nodata as u8 {
                                    continue;
                                }
                            }

                            output_image.put_pixel(
                                target_x as u32,
                                target_y as u32,
                                Rgba([r, g, b, 255]),
                            );
                        }
                        (tiff::ColorType::RGBA(_), DecodingResult::U8(data)) => {
                            let idx = (y_tile * data_width + x_tile) * 4;
                            let r = data[idx as usize];
                            let g = data[idx as usize + 1];
                            let b = data[idx as usize + 2];
                            let a = data[idx as usize + 3];
                            if let Some(nodata) = no_data {
                                if r == nodata as u8 || g == nodata as u8 || b == nodata as u8 {
                                    continue;
                                }
                            }
                            output_image.put_pixel(
                                target_x as u32,
                                target_y as u32,
                                Rgba([r, g, b, a]),
                            );
                        }
                        // Handle other color types or decoding results if necessary, or log a warning/error
                        _ => {
                            // Currently unsupported color type or bit depth for sub_region rendering
                            // Consider logging a warning or returning an error
                        }
                    };
                }
            }
        }
        // Resize the image to the requested output_size
        let resized_image = image::imageops::resize(
            &output_image,
            output_size,
            output_size,
            image::imageops::FilterType::Nearest, //todo should be a configure option
        );
        // Encode the resized image to PNG format
        let mut png_buffer = Vec::new();
        {
            let mut encoder = png::Encoder::new(
                BufWriter::new(&mut png_buffer),
                output_size, // Use output_size for the encoder dimensions
                output_size, // Use output_size for the encoder dimensions
            );
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(|e| {
                // Reusing existing CogError variants for PNG writing errors
                CogError::WritePngHeaderFailed(self.path.clone(), e)
            })?;
            writer
                .write_image_data(resized_image.as_raw()) // Write the resized image data
                .map_err(|e| CogError::WriteToPngFailed(self.path.clone(), e))?;
        }

        Ok(png_buffer) // Return the encoded PNG bytes
    }

    #[allow(clippy::cast_sign_loss)]
    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::too_many_lines)]
    pub fn get_tile(&self, xyz: TileCoord) -> MartinResult<TileData> {
        if self.force_google {
            if let Some(google_zoom) = self.meta.google_zoom {
                let google_min_zoom = google_zoom.0;
                let internal_zoom = self.meta.min_zoom + (xyz.z - google_min_zoom) as u8;
                if internal_zoom < self.meta.min_zoom || internal_zoom > self.meta.max_zoom {
                    return Ok(Vec::new());
                }
                let bbox = martin_tile_utils::xyz_to_bbox_webmercator(xyz.z, xyz.x, xyz.y, xyz.x, xyz.y);
                let tif_file =
                    File::open(&self.path).map_err(|e| FileError::IoError(e, self.path.clone()))?;
                let mut decoder = Decoder::new(tif_file)
                    .map_err(|e| CogError::InvalidTiffFile(e, self.path.clone()))?;
                decoder = decoder.with_limits(tiff::decoder::Limits::unlimited());

                let png_bytes = self.sub_region(&mut decoder, internal_zoom, bbox, 512)?;
                return Ok(png_bytes);
            } else {
                return Ok(Vec::new());
            }
        }

        if xyz.z < self.meta.min_zoom || xyz.z > self.meta.max_zoom {
            return Ok(Vec::new());
        }

        let tif_file =
            File::open(&self.path).map_err(|e| FileError::IoError(e, self.path.clone()))?;
        let mut decoder =
            Decoder::new(tif_file).map_err(|e| CogError::InvalidTiffFile(e, self.path.clone()))?;
        decoder = decoder.with_limits(tiff::decoder::Limits::unlimited());

        let ifd = self.meta.zoom_and_ifd.get(&(xyz.z)).ok_or_else(|| {
            CogError::ZoomOutOfRange(
                xyz.z,
                self.path.clone(),
                self.meta.min_zoom,
                self.meta.max_zoom,
            )
        })?;

        decoder
            .seek_to_image(*ifd)
            .map_err(|e| CogError::IfdSeekFailed(e, *ifd, self.path.clone()))?;

        let (across, down) = self
            .meta
            .zoom_and_tile_across_down
            .get(&(xyz.z))
            .ok_or_else(|| {
                CogError::ZoomOutOfRange(
                    xyz.z,
                    self.path.clone(),
                    self.meta.min_zoom,
                    self.meta.max_zoom,
                )
            })?;
        let tile_idx;
        if let Some(idx) = get_tile_idx(xyz, *across, *down) {
            tile_idx = idx;
        } else {
            return Ok(Vec::new());
        }
        let decode_result = decoder
            .read_chunk(tile_idx)
            .map_err(|e| CogError::ReadChunkFailed(e, tile_idx, *ifd, self.path.clone()))?;
        let color_type = decoder
            .colortype()
            .map_err(|e| CogError::InvalidTiffFile(e, self.path.clone()))?;

        let (tile_width, tile_height) = decoder.chunk_dimensions();
        let (data_width, data_height) = decoder.chunk_data_dimensions(tile_idx);

        //do more research on the not u8 case, is this the right way to do it?
        let png_file_bytes = match (decode_result, color_type) {
            (DecodingResult::U8(vec), tiff::ColorType::RGB(_)) => rgb_to_png(
                vec,
                (tile_width, tile_height),
                (data_width, data_height),
                3,
                self.meta.nodata.map(|v| v as u8),
                &self.path,
            ),
            (DecodingResult::U8(vec), tiff::ColorType::RGBA(_)) => rgb_to_png(
                vec,
                (tile_width, tile_height),
                (data_width, data_height),
                4,
                self.meta.nodata.map(|v| v as u8),
                &self.path,
            ),
            (_, _) => Err(CogError::NotSupportedColorTypeAndBitDepth(
                color_type,
                self.path.clone(),
            )),
            // do others in next PRs, a lot of disscussion would be needed
        }?;
        Ok(png_file_bytes)
    }
}

fn get_covered_tile_indexes(
    window: [f64; 4],
    cog_extent: [f64; 4],
    across_down: (u32, u32),
    cog_tile_size: (u32, u32),
    resolution: &[f64; 3],
) -> Vec<(u32, u32)> {
    let epsilon = 1e-6;

    let tile_span_x = f64::from(cog_tile_size.0) * resolution[0];
    // resolution[1] is typically negative, use its absolute value for span calculation
    let tile_span_y = f64::from(cog_tile_size.1) * resolution[1].abs();

    let tile_matrix_min_x = cog_extent[0];
    // Use max Y from extent as the top edge for row calculation
    let tile_matrix_max_y = cog_extent[3];

    let matrix_width = across_down.0;
    let matrix_height = across_down.1;

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

#[async_trait]
impl Source for CogSource {
    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_tilejson(&self) -> &TileJSON {
        &self.tilejson
    }

    fn get_tile_info(&self) -> TileInfo {
        self.tileinfo
    }

    fn clone_source(&self) -> Box<dyn Source> {
        Box::new(self.clone())
    }

    async fn get_tile(
        &self,
        xyz: TileCoord,
        _url_query: Option<&UrlQuery>,
    ) -> MartinResult<TileData> {
        Ok(self.get_tile(xyz)?)
    }
}

fn get_tile_idx(xyz: TileCoord, across: u32, down: u32) -> Option<u32> {
    if xyz.y >= down || xyz.x >= across {
        return None;
    }

    let tile_idx = xyz.y * across + xyz.x;
    if tile_idx >= across * down {
        return None;
    }
    Some(tile_idx)
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

fn verify_requirments(
    decoder: &mut Decoder<File>,
    model_info: &ModelInfo,
    path: &Path,
) -> Result<(), CogError> {
    let chunk_type = decoder.get_chunk_type();
    // see the requirement 2 in https://docs.ogc.org/is/21-026/21-026.html#_tiles
    if chunk_type != ChunkType::Tile {
        Err(CogError::NotSupportedChunkType(path.to_path_buf()))?;
    }

    // see https://docs.ogc.org/is/21-026/21-026.html#_planar_configuration_considerations and https://www.verypdf.com/document/tiff6/pg_0038.htm
    // we might support planar configuration 2 in the future
    decoder
        .get_tag_unsigned(Tag::PlanarConfiguration)
        .map_err(|e| {
            CogError::TagsNotFound(
                e,
                vec![Tag::PlanarConfiguration.to_u16()],
                0,
                path.to_path_buf(),
            )
        })
        .and_then(|config| {
            if config == 1 {
                Ok(())
            } else {
                Err(CogError::PlanarConfigurationNotSupported(
                    path.to_path_buf(),
                    0,
                    config,
                ))
            }
        })?;

    let color_type = decoder
        .colortype()
        .map_err(|e| CogError::InvalidTiffFile(e, path.to_path_buf()))?;

    if !matches!(
        color_type,
        tiff::ColorType::RGB(8) | tiff::ColorType::RGBA(8)
    ) {
        Err(CogError::NotSupportedColorTypeAndBitDepth(
            color_type,
            path.to_path_buf(),
        ))?;
    };

    match model_info {
        (Some(pixel_scale), Some(tie_points), _)
             =>
        {
            if (pixel_scale[0] + pixel_scale[1]).abs() > 0.01{
                Err(CogError::NonSquaredImage(path.to_path_buf(), pixel_scale[0], pixel_scale[1]))
            }
            else if pixel_scale.len() != 3 || tie_points.len() % 6 != 0 {
                Err(CogError::InvalidGeoInformation(path.to_path_buf(), "The length of pixel scale should be 3, and the length of tie points should be a multiple of 6".to_string()))
            }else{
                Ok(())
            }
       }
        (_, _, Some(matrix))
        => {
            if matrix.len() < 16 {
                Err(CogError::InvalidGeoInformation(path.to_path_buf(), "The length of matrix should be 16".to_string()))
        }else{
                Ok(())
        }
        },
            _ => Err(CogError::InvalidGeoInformation(path.to_path_buf(), "The model information is not found, either transformation (tag number 34264) or pixel scale(tag number 33550) && tie points(33922) should be inside ".to_string())),
    }?;

    Ok(())
}

#[allow(clippy::cast_possible_truncation)]
fn get_meta(path: &PathBuf) -> Result<Meta, FileError> {
    let tif_file = File::open(path).map_err(|e| FileError::IoError(e, path.clone()))?;
    let mut decoder = Decoder::new(tif_file)
        .map_err(|e| CogError::InvalidTiffFile(e, path.clone()))?
        .with_limits(tiff::decoder::Limits::unlimited());

    let gdal_metadata = decoder.get_tag_ascii_string(Tag::Unknown(42112));

    let model_info = get_model_infos(&mut decoder, path);
    verify_requirments(&mut decoder, &model_info, path)?;
    let tile_size = decoder.chunk_dimensions();
    let nodata: Option<f64> = if let Ok(no_data) = decoder.get_tag_ascii_string(GdalNodata) {
        no_data.parse().ok()
    } else {
        None
    };

    let pixel_scale = model_info.0;
    let tie_points = model_info.1;
    let transformations = model_info.2;

    let origin: [f64; 3] = get_origin(tie_points.as_deref(), transformations.as_deref(), path)?;

    let full_resolution =
        get_full_resolution(pixel_scale.as_deref(), transformations.as_deref(), path)?;
    let (full_width_pixel, full_length_pixel) = decoder.dimensions().map_err(|e| {
        CogError::TagsNotFound(
            e,
            vec![Tag::ImageWidth.to_u16(), Tag::ImageLength.to_u16()],
            0, // we are at ifd 0, the first image, haven't seek to others
            path.clone(),
        )
    })?;

    let full_width = full_resolution[0] * f64::from(full_width_pixel);
    let full_length = full_resolution[1] * f64::from(full_length_pixel);

    let extent = get_extent(
        transformations.as_deref(),
        &origin,
        (full_width_pixel, full_length_pixel),
        (full_width, full_length),
    );
    let mut zoom_and_ifd: HashMap<u8, usize> = HashMap::new();
    let mut zoom_and_tile_across_down: HashMap<u8, (u32, u32)> = HashMap::new();

    let mut resolutions: HashMap<u8, [f64; 3]> = HashMap::new();

    let images_ifd = get_images_ifd(&mut decoder, path);
    for (idx, image_ifd) in images_ifd.iter().enumerate() {
        decoder
            .seek_to_image(*image_ifd)
            .map_err(|e| CogError::IfdSeekFailed(e, *image_ifd, path.clone()))?;

        let zoom = u8::try_from(images_ifd.len() - (idx + 1))
            .map_err(|_| CogError::TooManyImages(path.clone()))?;

        let resolution = if *image_ifd == 0 {
            full_resolution
        } else {
            let (image_width, image_length) = decoder.dimensions().map_err(|e| {
                CogError::TagsNotFound(
                    e,
                    vec![Tag::ImageWidth.to_u16(), Tag::ImageLength.to_u16()],
                    *image_ifd,
                    path.clone(),
                )
            })?;

            let res_x = full_width / f64::from(image_width);
            let res_y = full_length / f64::from(image_length);

            [res_x, res_y, 0.0]
        };
        let (tiles_across, tiles_down) = get_grid_dims(&mut decoder, path, *image_ifd)?;

        zoom_and_ifd.insert(zoom, *image_ifd);
        zoom_and_tile_across_down.insert(zoom, (tiles_across, tiles_down));
        resolutions.insert(zoom, resolution);
    }

    if images_ifd.is_empty() {
        Err(CogError::NoImagesFound(path.clone()))?;
    }
    let min_zoom = 0;
    let max_zoom = images_ifd.len() as u8 - 1;

    let google_zoom_range = to_google_zoom_range(min_zoom, max_zoom, gdal_metadata);

    Ok(Meta {
        min_zoom: 0,
        max_zoom,
        google_zoom: google_zoom_range,
        zoom_and_resolutions: resolutions,
        tile_size,
        extent,
        origin,
        zoom_and_ifd,
        zoom_and_tile_across_down,
        nodata,
    })
}

fn get_extent(
    transformation: Option<&[f64]>,
    origin: &[f64],
    (full_width_pixel, full_height_pixel): (u32, u32),
    (full_width, full_height): (f64, f64),
) -> [f64; 4] {
    if let Some(matrix) = transformation {
        let corners = [
            [0, 0],
            [0, full_height_pixel],
            [full_width_pixel, 0],
            [full_width_pixel, full_height_pixel],
        ];
        let transformed = corners.map(|pixel| {
            let i = f64::from(pixel[0]);
            let j = f64::from(pixel[1]);
            let x = matrix[3] + (matrix[0] * i) + (matrix[1] * j);
            let y = matrix[7] + (matrix[4] * i) + (matrix[5] * j);
            (x, y)
        });
        let mut min_x = transformed[0].0;
        let mut min_y = transformed[1].1;
        let mut max_x = transformed[0].0;
        let mut max_y = transformed[1].1;
        for (x, y) in transformed {
            if x <= min_x {
                min_x = x;
            }
            if y <= min_y {
                min_y = y;
            }
            if x >= max_x {
                max_x = x;
            }
            if y >= max_y {
                max_y = y;
            }
        }
        return [min_x, min_y, max_x, max_y];
    }
    let x1 = origin[0];
    let y1 = origin[1];
    let x2 = x1 + full_width;
    let y2 = y1 + full_height;

    [x1.min(x2), y1.min(y2), x1.max(x2), y1.max(y2)]
}

fn get_full_resolution(
    pixel_scale: Option<&[f64]>,
    transformation: Option<&[f64]>,
    path: &Path,
) -> Result<[f64; 3], CogError> {
    match (pixel_scale, transformation) {
        (Some(scale), _) => Ok([scale[0], scale[1], scale[2]]),
        (_, Some(matrix)) => {
            if matrix[1] == 0.0 && matrix[4] == 0.0 {
                Ok([matrix[0], matrix[5], matrix[10]])
            } else {
                let x_res = (matrix[0] * matrix[0]) + (matrix[4] * matrix[4]);
                let y_res = ((matrix[1] * matrix[1]) + (matrix[5] * matrix[5])).sqrt() * -1.0;
                let z_res = matrix[10];
                Ok([x_res, y_res, z_res])
            }
        }
        (None, None) => Err(CogError::GetFullResolutionFailed(path.to_path_buf())),
    }
}

fn get_model_infos(decoder: &mut Decoder<File>, path: &Path) -> ModelInfo {
    let mut pixel_scale = decoder
        .get_tag_f64_vec(Tag::ModelPixelScaleTag)
        .map_err(|e| {
            CogError::TagsNotFound(
                e,
                vec![Tag::ModelPixelScaleTag.to_u16()],
                0,
                path.to_path_buf(),
            )
        })
        .ok();
    if let Some(pixel) = pixel_scale {
        pixel_scale = Some(vec![pixel[0], -pixel[1], pixel[2]]);
    }
    let tie_points = decoder
        .get_tag_f64_vec(Tag::ModelTiepointTag)
        .map_err(|e| {
            CogError::TagsNotFound(
                e,
                vec![Tag::ModelTiepointTag.to_u16()],
                0,
                path.to_path_buf(),
            )
        })
        .ok();
    let transformation = decoder
        .get_tag_f64_vec(Tag::ModelTransformationTag)
        .map_err(|e| {
            CogError::TagsNotFound(
                e,
                vec![Tag::ModelTransformationTag.to_u16()],
                0,
                path.to_path_buf(),
            )
        })
        .ok();
    (pixel_scale, tie_points, transformation)
}

fn get_grid_dims(
    decoder: &mut Decoder<File>,
    path: &Path,
    image_ifd: usize,
) -> Result<(u32, u32), FileError> {
    let (tile_width, tile_height) = (decoder.chunk_dimensions().0, decoder.chunk_dimensions().1);
    let (image_width, image_length) = get_image_dims(decoder, path, image_ifd)?;
    let tiles_across = image_width.div_ceil(tile_width);
    let tiles_down = image_length.div_ceil(tile_height);

    Ok((tiles_across, tiles_down))
}

fn get_image_dims(
    decoder: &mut Decoder<File>,
    path: &Path,
    image_ifd: usize,
) -> Result<(u32, u32), FileError> {
    let (image_width, image_length) = decoder.dimensions().map_err(|e| {
        CogError::TagsNotFound(
            e,
            vec![Tag::ImageWidth.to_u16(), Tag::ImageLength.to_u16()],
            image_ifd,
            path.to_path_buf(),
        )
    })?;

    Ok((image_width, image_length))
}

fn get_images_ifd(decoder: &mut Decoder<File>, path: &Path) -> Vec<usize> {
    let mut res = vec![];
    let mut ifd_idx = 0;
    loop {
        let is_image = decoder
            .get_tag_u32(Tag::NewSubfileType)
            .map_or_else(|_| true, |v| v & 4 != 4); // see https://www.verypdf.com/document/tiff6/pg_0036.htm
        if is_image {
            //todo We should not ignore mask in the next PRs
            res.push(ifd_idx);
        } else {
            warn!(
                "A subfile of {} is ignored in the tiff file as Martin currently does not support mask subfile in tiff. The ifd number of this subfile is {}",
                path.display(),
                ifd_idx
            );
        }

        ifd_idx += 1;

        let next_res = decoder.seek_to_image(ifd_idx);
        if next_res.is_err() {
            break;
        }
    }
    // how to get it sorted from big to little number
    res
}

// see https://docs.ogc.org/is/19-008r4/19-008r4.html#_geotiff_tags_for_coordinate_transformations
fn get_origin(
    tie_points: Option<&[f64]>,
    transformation: Option<&[f64]>,
    path: &Path,
) -> Result<[f64; 3], CogError> {
    match (tie_points, transformation) {
        (Some(points), _) if points.len() == 6 => Ok([points[3], points[4], points[5]]),
        (_, Some(matrix)) if matrix.len() >= 12 => Ok([matrix[3], matrix[7], matrix[11]]),
        _ => Err(CogError::GetOriginFailed(path.to_path_buf())),
    }
}

fn meta_to_tilejson(meta: &Meta) -> TileJSON {
    let min_zoom;
    let max_zoom;
    if let Some(google_zoom) = meta.google_zoom {
        min_zoom = google_zoom.0;
        max_zoom = google_zoom.1;
    } else {
        min_zoom = meta.min_zoom;
        max_zoom = meta.max_zoom;
    }

    let tilejson = tilejson! {
        tiles: vec![],
        minzoom: min_zoom,
        maxzoom:max_zoom,
    };
    tilejson
}

fn to_google_zoom_range(
    actual_min: u8,
    actual_max: u8,
    gdal_metadata: TiffResult<String>,
) -> Option<(u8, u8)> {
    let mut result = None;
    if let Ok(gdal_metadata) = gdal_metadata {
        let re_name = Regex::new(r#"<Item name="NAME" domain="TILING_SCHEME">([^<]+)</Item>"#);
        let re_zoom =
            Regex::new(r#"<Item name="ZOOM_LEVEL" domain="TILING_SCHEME">([^<]+)</Item>"#);

        let mut tiling_schema = None;
        if let Ok(re_name) = re_name {
            if let Some(caps) = re_name.captures(&gdal_metadata) {
                tiling_schema = Some(caps[1].to_string());
            }
        };

        let mut zoom_level: Option<u8> = None;
        if let Ok(re_zoom) = re_zoom {
            if let Some(caps) = re_zoom.captures(&gdal_metadata) {
                zoom_level = caps[1].parse().ok();
            }
        };

        if let Some(zoom) = zoom_level {
            if tiling_schema == Some("GoogleMapsCompatible".to_string()) {
                let google_min = zoom - actual_max + actual_min;
                result = Some((google_min, zoom));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use insta::{Settings, assert_yaml_snapshot};
    use martin_tile_utils::{TileCoord, xyz_to_bbox};
    use rstest::rstest;
    use std::{fs::File, io::Write, path::PathBuf};
    use tiff::decoder::Decoder;

    use crate::cog::source::{get_full_resolution, get_tile_idx};
    use approx::assert_abs_diff_eq;

    use super::{Meta, get_covered_tile_indexes, get_meta};

    #[test]
    fn can_calc_tile_idx() {
        assert_eq!(Some(0), get_tile_idx(TileCoord { z: 0, x: 0, y: 0 }, 3, 3));
        assert_eq!(Some(8), get_tile_idx(TileCoord { z: 0, x: 2, y: 2 }, 3, 3));
        assert_eq!(None, get_tile_idx(TileCoord { z: 0, x: 3, y: 0 }, 3, 3));
        assert_eq!(None, get_tile_idx(TileCoord { z: 0, x: 1, y: 9 }, 3, 3));
    }

    #[rstest]
    // the right half should be transprent
    #[case(
        "../tests/fixtures/cog/expected/right_padded.png",
        (0,0,0,None),None,(128,256),(256,256)
    )]
    // the down half should be transprent
    #[case(
        "../tests/fixtures/cog/expected/down_padded.png",
        (0,0,0,None),None,(256,128),(256,256)
    )]
    // the up half should be half transprent and down half should be transprent
    #[case(
        "../tests/fixtures/cog/expected/down_padded_with_alpha.png",
        (0,0,0,Some(128)),None,(256,128),(256,256)
    )]
    // the left half should be half transprent and the right half should be transprent
    #[case(
        "../tests/fixtures/cog/expected/right_padded_with_alpha.png",
        (0,0,0,Some(128)),None,(128,256),(256,256)
    )]
    // should be all half transprent
    #[case(
        "../tests/fixtures/cog/expected/not_padded.png",
        (0,0,0,Some(128)),None,(256,256),(256,256)
    )]
    // all padded and with a no_data whose value is 128, and all the component is 128
    // so that should be all transprent
    #[case(
        "../tests/fixtures/cog/expected/all_transprent.png",
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
        let componse_count = if components.3.is_some() { 4 } else { 3 };
        let png_bytes = super::rgb_to_png(
            pixels,
            (tile_width, tile_height),
            (data_width, data_height),
            componse_count,
            no_value,
            &PathBuf::from("not_exist.tif"),
        )
        .unwrap();
        let expected = std::fs::read(expected_file_path).unwrap();
        assert_eq!(png_bytes, expected);
    }

    #[rstest]
    #[case(
        None,Some(vec![0.0, 0.0, 0.0, 1_620_750.250_8, 4_277_012.715_3, 0.0]),
        [1_620_750.250_8, 4_277_012.715_3, 0.0]
    )]
    #[case(
        Some(vec![
            0.0, 100.0, 0.0, 400_000.0, 100.0, 0.0, 0.0, 500_000.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 1.0,
        ]),
        None,
        [400_000.0, 500_000.0, 0.0]
    )]
    fn can_get_origin(
        #[case] matrix: Option<Vec<f64>>,
        #[case] tie_point: Option<Vec<f64>>,
        #[case] expected: [f64; 3],
    ) {
        let origin = super::get_origin(
            tie_point.as_deref(),
            matrix.as_deref(),
            &PathBuf::from("not_exist.tif"),
        )
        .unwrap();
        assert_abs_diff_eq!(origin[0], expected[0]);
        assert_abs_diff_eq!(origin[1], expected[1]);
        assert_abs_diff_eq!(origin[2], expected[2]);
    }

    #[test]
    fn can_get_model_infos() {
        let path = PathBuf::from("../tests/fixtures/cog/rgb_u8.tif");
        let tif_file = File::open(&path).unwrap();
        let mut decoder = Decoder::new(tif_file).unwrap();

        let (pixel_scale, tie_points, transformation) = super::get_model_infos(&mut decoder, &path);

        assert_yaml_snapshot!(pixel_scale, @r###"
        - 10
        - -10
        - 0
        "###);
        assert_yaml_snapshot!(tie_points, @r###"
        - 0
        - 0
        - 0
        - 1620750.2508
        - 4277012.7153
        - 0
        "###);
        assert_yaml_snapshot!(transformation, @"~");
    }

    #[test]
    fn can_get_full_resolution() {
        let pixel_scale = Some(vec![10.000, -10.000, 0.000]);
        let transformation: Option<&[f64]> = None;

        let resolution = get_full_resolution(
            pixel_scale.as_deref(),
            transformation,
            &PathBuf::from("not_exist.tif"),
        )
        .ok();

        assert_yaml_snapshot!(resolution, @r###"
        - 10
        - -10
        - 0
        "###);
    }

    #[rstest]
    #[case(
        None,Some(vec![10.0,-10.0,0.0]),Some(vec![0.0, 0.0, 0.0, 1_620_750.250_8, 4_277_012.715_3, 0.0]),(512,512))
    ]
    #[case(
        Some(vec![
            10.0,0.0,0.0,1_620_750.250_8,
            0.0,-10.0,0.0,4_277_012.715_3,
            0.0,0.0,0.0,0.0,
            0.0,0.0,0.0,1.0
        ]),None,None,(512,512))
    ]
    fn can_get_extent(
        #[case] matrix: Option<Vec<f64>>,
        #[case] pixel_scale: Option<Vec<f64>>,
        #[case] tie_point: Option<Vec<f64>>,
        #[case] (full_width_pixel, full_length_pixel): (u32, u32),
    ) {
        use crate::cog::source::{get_extent, get_origin};

        let origin = get_origin(
            tie_point.as_deref(),
            matrix.as_deref(),
            &PathBuf::from("not_exist.tif"),
        )
        .unwrap();
        let full_resolution = get_full_resolution(
            pixel_scale.as_deref(),
            matrix.as_deref(),
            &PathBuf::from("not_exist.tif"),
        )
        .unwrap();

        let full_width = full_resolution[0] * f64::from(full_width_pixel);
        let full_length = full_resolution[1] * f64::from(full_length_pixel);

        let extent = get_extent(
            matrix.as_deref(),
            &origin,
            (full_width_pixel, full_length_pixel),
            (full_width, full_length),
        );

        assert_abs_diff_eq!(extent[0], 1_620_750.250_8);
        assert_abs_diff_eq!(extent[1], 4_271_892.715_3);
        assert_abs_diff_eq!(extent[2], 1_625_870.250_8);
        assert_abs_diff_eq!(extent[3], 4_277_012.715_3);
    }

    #[test]
    fn can_get_meta() {
        let path = PathBuf::from("../tests/fixtures/cog/rgb_u8.tif");

        let meta = super::get_meta(&path).unwrap();

        let mut settings = Settings::new();
        settings.set_sort_maps(true);

        // with this settings, the order of hashmap would be fixed to get a stable test
        settings.bind(|| {
            insta::assert_yaml_snapshot!(meta,@r###"
            min_zoom: 0
            max_zoom: 3
            google_zoom: ~
            origin:
              - 1620750.2508
              - 4277012.7153
              - 0
            extent:
              - 1620750.2508
              - 4271892.7153
              - 1625870.2508
              - 4277012.7153
            zoom_and_resolutions:
              0:
                - 80
                - -80
                - 0
              1:
                - 40
                - -40
                - 0
              2:
                - 20
                - -20
                - 0
              3:
                - 10
                - -10
                - 0
            zoom_and_ifd:
              0: 3
              1: 2
              2: 1
              3: 0
            zoom_and_tile_across_down:
              0:
                - 1
                - 1
              1:
                - 1
                - 1
              2:
                - 1
                - 1
              3:
                - 2
                - 2
            nodata: ~
            tile_size:
              - 256
              - 256
            "###);
        });
    }

    #[test]
    fn can_trans_to_google() {
        let path = PathBuf::from("../tests/fixtures/cog/google_compatible.tif");

        let source = super::CogSource::new("test".to_string(), path, true).unwrap();
        let window = [1620847.0, 4276072.0, 1621379.0, 4276545.0];
        let tif_file = File::open("../tests/fixtures/cog/google_compatible.tif").unwrap();
        let mut decoder = Decoder::new(tif_file).unwrap();
        let result = source.sub_region(&mut decoder, 2, window, 512).unwrap();
        let expected_bytes =
            std::fs::read("../tests/fixtures/cog/expected/sub_region.png").unwrap();
        assert_eq!(result, expected_bytes);
    }

    #[test]
    fn can_get_covered_tiles() {
        let path = PathBuf::from("../tests/fixtures/cog/google_compatible.tif");
        let meta = get_meta(&path).unwrap();

        let extent = meta.extent;
        let tile_size = meta.tile_size;
        for zoom in 0..=meta.max_zoom {
            let (tile_across, tile_down) = meta.zoom_and_tile_across_down[&zoom];
            let resolution = meta.zoom_and_resolutions[&zoom];

            for across in 0..tile_across {
                for down in 0..tile_down {
                    let window = calculate_tile_window(&meta, zoom, across, down);

                    let idx = get_covered_tile_indexes(
                        window,
                        extent,
                        (tile_across, tile_down),
                        tile_size,
                        &resolution,
                    );
                    assert_eq!(1, idx.len());
                    assert_eq!(across, idx[0].0);
                    assert_eq!(down, idx[0].1);
                }
            }
        }
    }

    // Helper function to calculate the geographic window of a tile
    fn calculate_tile_window(meta: &Meta, zoom: u8, across: u32, down: u32) -> [f64; 4] {
        let resolution = meta.zoom_and_resolutions[&zoom];
        let tile_size = meta.tile_size;

        let res_x = resolution[0];
        // Resolution Y is typically negative
        let res_y = resolution[1];

        let tile_width_geo = f64::from(tile_size.0) * res_x;
        let tile_height_geo = f64::from(tile_size.1) * res_y; // This will be negative

        let min_x = meta.origin[0] + f64::from(across) * tile_width_geo;
        let max_y = meta.origin[1] + f64::from(down) * tile_height_geo; // Top Y coordinate
        let max_x = min_x + tile_width_geo;
        let min_y = max_y + tile_height_geo; // Bottom Y coordinate

        // The window represents [min_x, min_y, max_x, max_y]
        [min_x, min_y, max_x, max_y]
    }
}

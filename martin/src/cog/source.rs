use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::vec;
use std::{fmt::Debug, path::PathBuf};

use log::warn;
use serde::Serialize;
use std::io::BufWriter;
use tiff::decoder::{ChunkType, Decoder, DecodingResult};
use tiff::tags::Tag::{self, GdalNodata};

use async_trait::async_trait;
use martin_tile_utils::{Format, TileCoord, TileInfo};
use tilejson::{tilejson, TileJSON};

use crate::file_config::FileError;
use crate::{file_config::FileResult, MartinResult, Source, TileData, UrlQuery};

use super::CogError;

type ModelInfo = (Option<Vec<f64>>, Option<Vec<f64>>, Option<Vec<f64>>);

#[derive(Clone, Debug, Serialize)]
struct Meta {
    min_zoom: u8,
    max_zoom: u8,
    resolutions: HashMap<u8, [f64; 3]>,
    zoom_and_ifd: HashMap<u8, usize>,
    zoom_and_tile_across_down: HashMap<u8, (u32, u32)>,
    nodata: Option<f64>,
    origin: [f64; 3],
    extent: [f64; 4],
}

#[derive(Clone, Debug)]
pub struct CogSource {
    id: String,
    path: PathBuf,
    meta: Meta,
    tilejson: TileJSON,
    tileinfo: TileInfo,
}

impl CogSource {
    pub fn new(id: String, path: PathBuf) -> FileResult<Self> {
        let tileinfo = TileInfo::new(Format::Png, martin_tile_utils::Encoding::Uncompressed);
        let meta = get_meta(&path)?;

        let tilejson: TileJSON = meta_to_tilejson(&meta);

        Ok(CogSource {
            id,
            path,
            meta,
            tilejson,
            tileinfo,
        })
    }
    #[allow(clippy::cast_sign_loss)]
    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::too_many_lines)]
    pub fn get_tile(&self, xyz: TileCoord) -> MartinResult<TileData> {
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
        };
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

fn meta_to_tilejson(meta: &Meta) -> TileJSON {
    let mut tilejson = tilejson! {
        tiles: vec![],
        minzoom: meta.min_zoom,
        maxzoom: meta.max_zoom
    };

    let mut cog_info = serde_json::Map::new();

    cog_info.insert(
        "minZoom".to_string(),
        serde_json::Value::from(meta.min_zoom),
    );

    cog_info.insert(
        "maxZoom".to_string(),
        serde_json::Value::from(meta.max_zoom),
    );

    let mut resolutions_map = serde_json::Map::new();
    for (key, value) in &meta.resolutions {
        resolutions_map.insert(
            key.to_string(),                            // Convert u8 key to String
            serde_json::Value::from(value.to_vec()[0]), // Convert [f64; 3] to Vec<f64> and then to serde_json::Value
        );
    }

    cog_info.insert(
        "resolutions".to_string(),
        serde_json::Value::from(resolutions_map),
    );

    cog_info.insert(
        "origin".to_string(),
        serde_json::Value::from(meta.origin.to_vec()),
    );

    cog_info.insert(
        "extent".to_string(),
        serde_json::Value::from(meta.extent.to_vec()),
    );

    tilejson
        .other
        .insert("cog_custom_grid".to_string(), serde_json::json!(cog_info));
    tilejson
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
            if pixel_scale.len() != 3 || tie_points.len() % 6 != 0 {
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

    let model_info = get_model_infos(&mut decoder, path);
    verify_requirments(&mut decoder, &model_info, path)?;
    let nodata: Option<f64> = if let Ok(no_data) = decoder.get_tag_ascii_string(GdalNodata) {
        no_data.parse().ok()
    } else {
        None
    };

    let pixel_scale = model_info.0;
    let tie_points = model_info.1;
    let transformations = model_info.2;

    let origin = get_origin(tie_points.as_deref(), transformations.as_deref(), path)?;

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

    Ok(Meta {
        min_zoom: 0,
        max_zoom: images_ifd.len() as u8 - 1,
        resolutions,
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
        let transed = corners.map(|pixel| {
            let i = f64::from(pixel[0]);
            let j = f64::from(pixel[1]);
            let x = matrix[3] + (matrix[0] * i) + (matrix[1] * j);
            let y = matrix[7] + (matrix[4] * i) + (matrix[5] * j);
            (x, y)
        });
        let mut min_x = transed[0].0;
        let mut min_y = transed[1].1;
        let mut max_x = transed[0].0;
        let mut max_y = transed[1].1;
        for (x, y) in transed {
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

#[cfg(test)]
mod tests {
    use insta::{assert_yaml_snapshot, Settings};
    use martin_tile_utils::TileCoord;
    use rstest::rstest;
    use std::{fs::File, path::PathBuf};
    use tiff::decoder::Decoder;

    use crate::cog::source::{get_full_resolution, get_tile_idx};
    use approx::assert_abs_diff_eq;

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

    #[test]
    fn can_get_origin() {
        let matrix: Option<&[f64]> = None;
        let tie_point = Some(vec![0.0, 0.0, 0.0, 1_620_750.250_8, 4_277_012.715_3, 0.0]);

        let origin = super::get_origin(
            tie_point.as_deref(),
            matrix,
            &PathBuf::from("not_exist.tif"),
        )
        .unwrap();
        assert_abs_diff_eq!(origin[0], 1_620_750.250_8);
        assert_abs_diff_eq!(origin[1], 4_277_012.715_3);
        assert_abs_diff_eq!(origin[2], 0.0);
        // assert_eq!(origin, [1_620_750.250_8, 4_277_012.715_3, 0.0]);
        //todo add a test for matrix either in this PR.
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

    #[test]
    fn can_get_resolutions() {
        let path = PathBuf::from("../tests/fixtures/cog/rgb_u8.tif");

        let meta = super::get_meta(&path).unwrap();

        let mut settings = Settings::new();
        settings.set_sort_maps(true);

        // with this settings, the order of hashmap would be fixed to get a stable test
        settings.bind(|| {
            insta::assert_yaml_snapshot!(meta,@r###"
            min_zoom: 0
            max_zoom: 3
            resolutions:
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
            origin:
              - 1620750.2508
              - 4277012.7153
              - 0
            extent:
              - 1620750.2508
              - 4271892.7153
              - 1625870.2508
              - 4277012.7153
            "###);
        });
    }
}

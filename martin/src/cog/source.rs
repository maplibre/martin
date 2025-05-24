use std::collections::HashMap;
use std::fmt::Debug;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::vec;

use async_trait::async_trait;
use log::warn;
use martin_tile_utils::{Format, TileCoord, TileInfo};
use tiff::decoder::{ChunkType, Decoder, DecodingResult};
use tiff::tags::Tag::{self, GdalNodata};
use tilejson::{TileJSON, tilejson};

use super::CogError;
use super::model::ModelInfo;
use crate::file_config::{FileError, FileResult};
use crate::{MartinResult, Source, TileData, UrlQuery};

#[allow(dead_code)] // the unused model would be used in next PRs
#[derive(Clone, Debug)]
struct Meta {
    min_zoom: u8,
    max_zoom: u8,
    model: ModelInfo,
    // The geo coords of pixel(0, 0, 0) ordering in [x, y, z]
    origin: [f64; 3],
    // [minx, miny, maxx, maxy] in its model space coordinate system
    extent: [f64; 4],
    zoom_and_ifd: HashMap<u8, usize>,
    zoom_and_tile_across_down: HashMap<u8, (u32, u32)>,
    nodata: Option<f64>,
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
        let tilejson = tilejson! {
            tiles: vec![],
            minzoom: meta.min_zoom,
            maxzoom: meta.max_zoom
        };
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

fn verify_requirements(
    decoder: &mut Decoder<File>,
    model: &ModelInfo,
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
    }

    match (&model.pixel_scale, &model.tie_points, &model.transformation) {
        (Some(pixel_scale), Some(tie_points), _)
             =>
        {
            if pixel_scale.len() != 3 {
                Err(CogError::InvalidGeoInformation(path.to_path_buf(), "The count of pixel scale should be 3".to_string()))
            }
            else if (pixel_scale[0].abs() - pixel_scale[1].abs()).abs() > 0.01{
                Err(CogError::NonSquaredImage(path.to_path_buf(), pixel_scale[0], pixel_scale[1]))
            }
            else if tie_points.len() % 6 != 0 {
                Err(CogError::InvalidGeoInformation(path.to_path_buf(), "The count of tie points should be a multiple of 6".to_string()))
            }else{
                Ok(())
            }
       }
        (_, _, Some(matrix))
        => {
            if matrix.len() == 16 {
                Ok(())
            } else {
                Err(CogError::InvalidGeoInformation(path.to_path_buf(), "The length of matrix should be 16".to_string()))
            }
        },
            _ => Err(CogError::InvalidGeoInformation(path.to_path_buf(), "Either a valid transformation (tag 34264) or both pixel scale (tag 33550) and tie points (tag 33922) must be provided".to_string())),
    }?;

    Ok(())
}

#[allow(clippy::cast_possible_truncation)]
fn get_meta(path: &PathBuf) -> Result<Meta, FileError> {
    let tif_file = File::open(path).map_err(|e| FileError::IoError(e, path.clone()))?;
    let mut decoder = Decoder::new(tif_file)
        .map_err(|e| CogError::InvalidTiffFile(e, path.clone()))?
        .with_limits(tiff::decoder::Limits::unlimited());
    let model = ModelInfo::decode(&mut decoder, path);
    let origin = get_origin(
        model.tie_points.as_deref(),
        model.transformation.as_deref(),
        path,
    )?;
    let (full_width_pixel, full_length_pixel) = decoder.dimensions().map_err(|e| {
        CogError::TagsNotFound(
            e,
            vec![Tag::ImageWidth.to_u16(), Tag::ImageLength.to_u16()],
            0, // we are at ifd 0, the first image, haven't seek to others
            path.clone(),
        )
    })?;
    let full_resolution = get_full_resolution(
        model.pixel_scale.as_deref(),
        model.transformation.as_deref(),
        path,
    )?;
    let full_width = full_resolution[0] * f64::from(full_width_pixel);
    let full_length = full_resolution[1] * f64::from(full_length_pixel);
    let extent = get_extent(
        &origin,
        model.transformation.as_deref(),
        (full_width_pixel, full_length_pixel),
        (full_width, full_length),
    );
    verify_requirements(&mut decoder, &model, path)?;
    let mut zoom_and_ifd: HashMap<u8, usize> = HashMap::new();
    let mut zoom_and_tile_across_down: HashMap<u8, (u32, u32)> = HashMap::new();

    let nodata: Option<f64> = if let Ok(no_data) = decoder.get_tag_ascii_string(GdalNodata) {
        no_data.parse().ok()
    } else {
        None
    };

    let images_ifd = get_images_ifd(&mut decoder, path);

    for (idx, image_ifd) in images_ifd.iter().enumerate() {
        decoder
            .seek_to_image(*image_ifd)
            .map_err(|e| CogError::IfdSeekFailed(e, *image_ifd, path.clone()))?;

        let zoom = u8::try_from(images_ifd.len() - (idx + 1))
            .map_err(|_| CogError::TooManyImages(path.clone()))?;

        let (tiles_across, tiles_down) = get_grid_dims(&mut decoder, path, *image_ifd)?;

        zoom_and_ifd.insert(zoom, *image_ifd);
        zoom_and_tile_across_down.insert(zoom, (tiles_across, tiles_down));
    }

    if images_ifd.is_empty() {
        Err(CogError::NoImagesFound(path.clone()))?;
    }

    Ok(Meta {
        min_zoom: 0,
        max_zoom: images_ifd.len() as u8 - 1,
        model,
        origin,
        extent,
        zoom_and_ifd,
        zoom_and_tile_across_down,
        nodata,
    })
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
    res
}

fn get_origin(
    tie_points: Option<&[f64]>,
    transformation: Option<&[f64]>,
    path: &Path,
) -> Result<[f64; 3], CogError> {
    // From geotiff spec: "This matrix tag should not be used if the ModelTiepointTag and the ModelPixelScaleTag are already defined"
    // See more in https://docs.ogc.org/is/19-008r4/19-008r4.html#_geotiff_tags_for_coordinate_transformations
    match (tie_points, transformation) {
        // From geotiff spec: "If possible, the first tiepoint placed in this tag shall be the one establishing the location of the point (0,0) in raster space"
        (Some(points), _) if points.len() >= 6 => Ok([points[3], points[4], points[5]]),

        // coords =     matrix  * coords
        // |- -|     |-       -|  |- -|
        // | X |     | a b c d |  | I |
        // | | |     |         |  |   |
        // | Y |     | e f g h |  | J |
        // |   |  =  |         |  |   |
        // | Z |     | i j k l |  | K |
        // | | |     |         |  |   |
        // | 1 |     | m n o p |  | 1 |
        // |- -|     |-       -|  |- -|

        // The (I,J,K) of origin is (0,0,0), so:
        //
        //    x = I*a + J*b + K*c + 1*d => d => matrix[3]
        //    y = I*e + J*f + k*g + 1*h => h => matrix[7]
        //    z = I*i + J*j + K*k + 1*l => l => matrix[11]
        (_, Some(matrix)) if matrix.len() >= 12 => Ok([matrix[3], matrix[7], matrix[11]]),
        _ => Err(CogError::GetOriginFailed(path.to_path_buf())),
    }
}

fn get_full_resolution(
    pixel_scale: Option<&[f64]>,
    transformation: Option<&[f64]>,
    path: &Path,
) -> Result<[f64; 2], CogError> {
    match (pixel_scale, transformation) {
        // ModelPixelScaleTag = (ScaleX, ScaleY, ScaleZ)
        (Some(scale), _) => Ok([scale[0], scale[1]]),
        (_, Some(matrix)) => {
            let mut x_res =
                (matrix[0] * matrix[0] + matrix[4] * matrix[4] + matrix[8] * matrix[8]).sqrt();
            x_res = x_res.copysign(matrix[0]);
            let mut y_res =
                (matrix[1] * matrix[1] + matrix[5] * matrix[5] + matrix[9] * matrix[9]).sqrt();
            // A positive y_res indicates that model space Y cordinates decrease as raster space J indices increase. This is the standard vertical relationship between raster space and model space
            y_res = y_res.copysign(-matrix[5]);
            Ok([x_res, y_res]) // drop the z scale directly as we don't use it
        }
        (None, None) => Err(CogError::GetFullResolutionFailed(path.to_path_buf())),
    }
}

fn raster2model(i: u32, j: u32, matrix: &[f64]) -> (f64, f64) {
    let i = f64::from(i);
    let j = f64::from(j);
    let x = matrix[3] + (matrix[0] * i) + (matrix[1] * j);
    let y = matrix[7] + (matrix[4] * i) + (matrix[5] * j);
    (x, y)
}

fn get_extent(
    origin: &[f64; 3],
    transformation: Option<&[f64]>,
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
        let transformed = corners.map(|pixel| raster2model(pixel[0], pixel[1], matrix));
        let max_x = transformed.iter().map(|(x,_)|*x).max_by(f64::total_cmp).expect("corners has >1 elements and thus has immer a max");
        let min_x = transformed.iter().map(|(x,_)|*x).min_by(f64::total_cmp).expect("corners has >1 elements and thus has immer a min");
        let max_y = transformed.iter().map(|(y,_)|*y).max_by(f64::total_cmp).expect("corners has >1 elements and thus has immer a max");
        let min_y = transformed.iter().map(|(y,_)|*y).min_by(f64::total_cmp).expect("corners has >1 elements and thus has immer a min");
        return [min_x, min_y, max_x, max_y];
    }
    let [x1,y1,_] = origin;
    let x2 = x1 + full_width;
    let y2 = y1 + full_height;

    [x1.min(x2), y1.min(y2), x1.max(x2), y1.max(y2)]
}

#[cfg(test)]
mod tests {
    use std::{fs::File, path::PathBuf};

    use insta::assert_yaml_snapshot;
    use martin_tile_utils::TileCoord;
    use rstest::rstest;
    use tiff::decoder::Decoder;

    use crate::cog::{model::ModelInfo, source::get_tile_idx};

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
    fn can_get_model_infos() {
        let path = PathBuf::from("../tests/fixtures/cog/rgb_u8.tif");
        let tif_file = File::open(&path).unwrap();
        let mut decoder = Decoder::new(tif_file).unwrap();

        let model = ModelInfo::decode(&mut decoder, &path);
        let (pixel_scale, tie_points, transformation) =
            (model.pixel_scale, model.tie_points, model.transformation);
        assert_yaml_snapshot!(pixel_scale, @r###"
        - 10
        - 10
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

    #[rstest]
    #[case(
        Some(vec![0.0, 0.0, 0.0, 1_620_750.250_8, 4_277_012.715_3, 0.0]),None,
        Some([1_620_750.250_8, 4_277_012.715_3, 0.0])
    )]
    #[case(
        None,Some(vec![
            0.0, 100.0, 0.0, 400_000.0, 100.0, 0.0, 0.0, 500_000.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 1.0,
        ]),
        Some([400_000.0, 500_000.0, 0.0])
    )]
    #[case(None, None, None)]
    fn can_get_origin(
        #[case] tie_point: Option<Vec<f64>>,
        #[case] matrix: Option<Vec<f64>>,
        #[case] expected: Option<[f64; 3]>,
    ) {
        use approx::assert_abs_diff_eq;

        let origin = super::get_origin(
            tie_point.as_deref(),
            matrix.as_deref(),
            &PathBuf::from("not_exist.tif"),
        )
        .ok();
        match (origin, expected) {
            (Some(o), Some(e)) => {
                assert_abs_diff_eq!(o[0], e[0]);
                assert_abs_diff_eq!(o[1], e[1]);
                assert_abs_diff_eq!(o[2], e[2]);
            }
            (None, None) => {
                // Both are None, which is expected
            }
            _ => {
                panic!("Origin {origin:?} does not match expected {expected:?}");
            }
        }
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
        use approx::assert_abs_diff_eq;

        use crate::cog::source::{get_extent, get_full_resolution, get_origin};

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
            &origin,
            matrix.as_deref(),
            (full_width_pixel, full_length_pixel),
            (full_width, full_length),
        );

        assert_abs_diff_eq!(extent[0], 1_620_750.250_8);
        assert_abs_diff_eq!(extent[1], 4_271_892.715_3);
        assert_abs_diff_eq!(extent[2], 1_625_870.250_8);
        assert_abs_diff_eq!(extent[3], 4_277_012.715_3);
    }

    #[rstest]
    #[case(
        None,Some(vec![118.4505876 , 118.4505876, 0.0]),[118.4505876,118.4505876, 0.0]
    )]
    fn can_get_full_resolution(
        #[case] matrix: Option<Vec<f64>>,
        #[case] pixel_scale: Option<Vec<f64>>,
        #[case] expected: [f64; 3],
    ) {
        use approx::assert_abs_diff_eq;

        use crate::cog::source::get_full_resolution;

        let full_resolution = get_full_resolution(
            pixel_scale.as_deref(),
            matrix.as_deref(),
            &PathBuf::from("not_exist.tif"),
        )
        .unwrap();
        assert_abs_diff_eq!(full_resolution[0], expected[0]);
        assert_abs_diff_eq!(full_resolution[1], expected[1]);
    }
}

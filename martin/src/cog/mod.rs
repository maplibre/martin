mod errors;

pub use errors::CogError;
use log::warn;
use regex::Regex;
use tiff::TiffResult;

use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::vec;
use std::{fmt::Debug, path::PathBuf};

use std::io::BufWriter;
use tiff::decoder::{ChunkType, Decoder, DecodingResult};
use tiff::tags::Tag::{self, GdalNodata};

use async_trait::async_trait;
use martin_tile_utils::{Format, TileCoord, TileInfo};
use serde::{Deserialize, Serialize};
use tilejson::{tilejson, TileJSON};
use url::Url;

use crate::file_config::FileError;
use crate::{
    config::UnrecognizedValues,
    file_config::{ConfigExtras, FileResult, SourceConfigExtras},
    MartinResult, Source, TileData, UrlQuery,
};

type Mapping = (u8, usize, u32, u32);
type GeoTransformations<'a> = (
    Option<&'a Vec<f64>>,
    Option<&'a Vec<f64>>,
    Option<&'a Vec<f64>>,
);
pub const EARTH_CIRCUMFERENCE: f64 = 40_075_016.685_578_5;

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
        let mut min_zoom = meta.min_zoom;
        let mut max_zoom = meta.max_zoom;
        if let Some(google_compablity) = &meta.google_compatiblity {
            min_zoom = google_compablity.google_zoom.0;
            max_zoom = google_compablity.google_zoom.1;
        }
        let tilejson = tilejson! {
            tiles: vec![],
            minzoom: min_zoom,
            maxzoom: max_zoom
        };
        Ok(CogSource {
            id,
            path,
            meta,
            tilejson,
            tileinfo,
        })
    }
}

#[derive(Clone, Debug)]
struct Meta {
    min_zoom: u8,
    max_zoom: u8,
    zoom_and_ifd: HashMap<u8, usize>,
    zoom_and_tile_across_down: HashMap<u8, (u32, u32)>,
    google_compatiblity: Option<GoogleMapping>,
    nodata: Option<f64>,
}

#[derive(Clone, Debug)]
struct GoogleMapping {
    google_zoom: (u8, u8),
    // key: google_zoom
    // Value: actual_zoom,ifd_number;
    // And the google_x, google_y when actual_x = 0, actual_y = 0, eg, the google tile index of the top left corner tile
    mapping: HashMap<u8, Mapping>,
}
impl GoogleMapping {
    pub fn to_actual(&self, input_google: TileCoord) -> Option<(u8, usize, u32, u32)> {
        let mut result = None;
        if let Some((actual_zoom, ifd_number, google_x, google_y)) =
            self.mapping.get(&input_google.z)
        {
            let actual_x = input_google.x - google_x;
            let actual_y = input_google.y - google_y;
            result = Some((*actual_zoom, *ifd_number, actual_x, actual_y));
        }
        result
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

    #[allow(clippy::cast_sign_loss)]
    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::too_many_lines)]
    async fn get_tile(
        &self,
        xyz: TileCoord,
        _url_query: Option<&UrlQuery>,
    ) -> MartinResult<TileData> {
        let tif_file =
            File::open(&self.path).map_err(|e| FileError::IoError(e, self.path.clone()))?;
        let mut decoder =
            Decoder::new(tif_file).map_err(|e| CogError::InvalidTiffFile(e, self.path.clone()))?;
        decoder = decoder.with_limits(tiff::decoder::Limits::unlimited());

        let (zoom_level, ifd, x, y) = if let Some(google) = &self.meta.google_compatiblity {
            google.to_actual(xyz).ok_or_else(|| {
                CogError::ZoomOutOfRange(
                    xyz.z,
                    self.path.clone(),
                    self.meta.min_zoom,
                    self.meta.max_zoom,
                )
            })?
        } else {
            let ifd_number = *self.meta.zoom_and_ifd.get(&(xyz.z)).ok_or_else(|| {
                CogError::ZoomOutOfRange(
                    xyz.z,
                    self.path.clone(),
                    self.meta.min_zoom,
                    self.meta.max_zoom,
                )
            })?;
            (xyz.z, ifd_number, xyz.x, xyz.y)
        };
        decoder
            .seek_to_image(ifd)
            .map_err(|e| CogError::IfdSeekFailed(e, ifd, self.path.clone()))?;

        let tiles_across = self
            .meta
            .zoom_and_tile_across_down
            .get(&(zoom_level))
            .ok_or_else(|| {
                CogError::ZoomOutOfRange(
                    zoom_level,
                    self.path.clone(),
                    self.meta.min_zoom,
                    self.meta.max_zoom,
                )
            })?
            .0;
        let tile_idx = y * tiles_across + x;
        let decode_result = decoder
            .read_chunk(tile_idx)
            .map_err(|e| CogError::ReadChunkFailed(e, tile_idx, ifd, self.path.clone()))?;
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

fn rgb_to_png(
    vec: Vec<u8>,
    (tile_width, tile_height): (u32, u32),
    (data_width, data_height): (u32, u32),
    chunk_components_count: u32,
    nodata: Option<u8>,
    path: &Path,
) -> Result<Vec<u8>, CogError> {
    let is_padded = data_width != tile_width;
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CogConfig {
    #[serde(flatten)]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigExtras for CogConfig {
    fn get_unrecognized(&self) -> &UnrecognizedValues {
        &self.unrecognized
    }
}

impl SourceConfigExtras for CogConfig {
    async fn new_sources(&self, id: String, path: PathBuf) -> FileResult<Box<dyn Source>> {
        let cog = CogSource::new(id, path)?;
        Ok(Box::new(cog))
    }

    #[allow(clippy::no_effect_underscore_binding)]
    async fn new_sources_url(&self, _id: String, _url: Url) -> FileResult<Box<dyn Source>> {
        unreachable!()
    }

    fn parse_urls() -> bool {
        false
    }
}
fn verify_requirments(decoder: &mut Decoder<File>, path: &Path) -> Result<(), CogError> {
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
    Ok(())
}

#[allow(clippy::cast_possible_truncation)]
fn get_meta(path: &PathBuf) -> Result<Meta, FileError> {
    let tif_file = File::open(path).map_err(|e| FileError::IoError(e, path.clone()))?;
    let mut decoder = Decoder::new(tif_file)
        .map_err(|e| CogError::InvalidTiffFile(e, path.clone()))?
        .with_limits(tiff::decoder::Limits::unlimited());

    verify_requirments(&mut decoder, path)?;
    let mut zoom_and_ifd: HashMap<u8, usize> = HashMap::new();
    let mut zoom_and_tile_across_down: HashMap<u8, (u32, u32)> = HashMap::new();

    let nodata: Option<f64> = if let Ok(no_data) = decoder.get_tag_ascii_string(GdalNodata) {
        no_data.parse().ok()
    } else {
        None
    };

    let chunk_size = decoder.chunk_dimensions().0;
    let gdal_metadata = decoder.get_tag_ascii_string(Tag::Unknown(42112));
    let model_transformation = decoder.get_tag_f64_vec(Tag::ModelTransformationTag).ok();
    let model_tiepoint = decoder.get_tag_f64_vec(Tag::ModelTiepointTag).ok();
    let pixel_scale = decoder.get_tag_f64_vec(Tag::ModelPixelScaleTag).ok();

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
    let min_zoom = 0;
    let max_zoom = images_ifd.len() as u8 - 1;
    let mut google_mapping = None;

    let google_zooms = to_google_zoom_range(min_zoom, max_zoom, gdal_metadata);
    if let Some(google_zoom) = google_zooms {
        let mappings = get_google_mapping(
            &zoom_and_ifd,
            max_zoom,
            (google_zoom.0, google_zoom.1),
            chunk_size,
            (
                model_transformation.as_ref(),
                model_tiepoint.as_ref(),
                pixel_scale.as_ref(),
            ),
            path,
        )?;

        google_mapping = Some(GoogleMapping {
            google_zoom,
            mapping: mappings,
        });
    }
    Ok(Meta {
        min_zoom,
        max_zoom,
        zoom_and_ifd,
        zoom_and_tile_across_down,
        google_compatiblity: google_mapping,
        nodata,
    })
}

fn get_google_mapping(
    zoom_and_ifd: &HashMap<u8, usize>,
    actual_max_zoom: u8,
    (google_min_zoom, google_max_zoom): (u8, u8),
    chunk_size: u32,
    (model_transformation, model_tiepoint, pixel_scale): GeoTransformations,
    path: &Path,
) -> Result<HashMap<u8, Mapping>, CogError> {
    let mut mappings = HashMap::new();
    for actual_zoom in zoom_and_ifd.keys() {
        let ifd_number = zoom_and_ifd
            .get(actual_zoom)
            .ok_or_else(|| CogError::IfdNotFound(*actual_zoom, path.to_path_buf(), *actual_zoom))?;
        let google_zoom = actual_zoom
            .checked_add(google_max_zoom)
            .and_then(|sum| sum.checked_sub(actual_max_zoom))
            .ok_or_else(|| CogError::GoogleZoomMappingFailed(
                format!("Google zoom mapping failed when trying to get the google zoom for actual zoom: {actual_zoom}, and the google zoom range is {google_min_zoom} - {google_max_zoom}, the actual zoom range is 0 - {actual_max_zoom}"), path.to_path_buf()
            ))?;

        let zoom_diff = u32::from(actual_max_zoom)
            .checked_sub(u32::from(*actual_zoom))
            .ok_or_else(|| CogError::GoogleZoomMappingFailed(
                format!("Google zoom mapping failed when trying to get the zoom difference between the actual zoom and the max zoom, the actual zoom is {actual_zoom}, and the actual zoom range is 0 - {actual_max_zoom}"), path.to_path_buf()
            ))?;
        let size_related = chunk_size
            .checked_mul(2_u32.pow(zoom_diff))
            .ok_or_else(|| {
                CogError::SizeRelatedFailed(*actual_zoom, zoom_diff, chunk_size, path.to_path_buf())
            })?;
        let center_pixel = (f64::from(size_related) / 2.0, f64::from(size_related) / 2.0);
        let center_pixel = pixel_to_model(
            model_transformation,
            model_tiepoint,
            pixel_scale,
            center_pixel.0,
            center_pixel.1,
            path.to_path_buf(),
        )?;

        let tile_idx = tile_index(center_pixel.0, center_pixel.1, google_zoom);
        mappings.insert(
            google_zoom,
            (*actual_zoom, *ifd_number, tile_idx.0, tile_idx.1),
        );
    }
    Ok(mappings)
}

/*
The transformation from pixel space (I,J,K) to model space (X,Y,Z) is given by the following equations:
    model                  image
    coords =     matrix  * coords
    |- -|     |-       -|  |- -|
    | X |     | a b c d |  | I |
    | | |     |         |  |   |
    | Y |     | e f g h |  | J |
    |   |  =  |         |  |   |
    | Z |     | i j k l |  | K |
    | | |     |         |  |   |
    | 1 |     | m n o p |  | 1 |
    |- -|     |-       -|  |- -|
By convention, and without loss of generality, the following parameters are currently hard-coded and will always be the same (but must be specified nonetheless):
    m = n = o = 0, p = 1.
And When the model space is 2-D, the matrix will have the more limited form:
    |- -|   |-       -| |- -|
    | X |   | a b 0 d | | I |
    | | |   |         | |   |
    | Y |   | e f 0 h | | J |
    |   | = |         | |   |
    | Z |   | 0 0 0 0 | | K |
    | | |   |         | |   |
    | 1 |   | 0 0 0 1 | | 1 |
    |- -|   |-       -| |- -|
*/
// see https://docs.ogc.org/is/19-008r4/19-008r4.html#_geotiff_tags_for_coordinate_transformations
fn pixel_to_model(
    model_transformation: Option<&Vec<f64>>,
    model_tiepoint: Option<&Vec<f64>>,
    pixel_scale: Option<&Vec<f64>>,
    pixcel_i: f64,
    pixel_j: f64,
    path: PathBuf,
) -> Result<(f64, f64), CogError> {
    let (x, y) = if let Some(transform) = model_transformation {
        // Using model transformation
        let center_x = transform[3] + (transform[0] * pixcel_i) + (transform[1] * pixel_j);
        let center_y = transform[7] + (transform[4] * pixcel_i) + (transform[5] * pixel_j);
        (center_x, center_y)
    } else if let (Some(tiepoint), Some(scale)) = (model_tiepoint, pixel_scale) {
        // Using tiepoint and pixel scale
        //todo add geotiff spec as comment
        let scale_x = scale[0];
        let scale_y = scale[1];
        let tx = tiepoint[3];
        let ty = tiepoint[4];
        let center_x = tx + pixcel_i * scale_x;
        let center_y = ty - pixel_j * scale_y;
        (center_x, center_y)
    } else {
        return Err(CogError::MissingGeospatialInfo(path));
    };

    Ok((x, y))
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

/// Convert web mercator x and y to tile index for a given zoom
#[must_use]
#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_sign_loss)]
pub fn tile_index(x: f64, y: f64, zoom: u8) -> (u32, u32) {
    let tile_size = EARTH_CIRCUMFERENCE / f64::from(1_u32 << zoom);
    let col = (((x - (EARTH_CIRCUMFERENCE * -0.5)).abs() / tile_size) as u32).min((1 << zoom) - 1);
    let row = ((((EARTH_CIRCUMFERENCE * 0.5) - y).abs() / tile_size) as u32).min((1 << zoom) - 1);
    (col, row)
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

mod errors;

use bytemuck::NoUninit;
pub use errors::CogError;
use png::{BitDepth, ColorType};
use regex::Regex;

use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::vec;
use std::{fmt::Debug, path::PathBuf};

use std::io::BufWriter;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;

use async_trait::async_trait;
use martin_tile_utils::{Format, TileCoord, TileInfo};
use serde::{Deserialize, Serialize};
use tilejson::{tilejson, TileJSON};
use url::Url;

extern crate bytemuck;
extern crate tilejson;

use crate::file_config::FileError;
use crate::{
    config::UnrecognizedValues,
    file_config::{ConfigExtras, FileResult, SourceConfigExtras},
    MartinResult, Source, TileData, UrlQuery,
};

#[derive(Clone, Debug)]
pub struct CogSource {
    id: String,
    path: PathBuf,
    meta: Meta,
    tilejson: TileJSON,
    tileinfo: TileInfo,
}

#[derive(Clone, Debug)]
struct Meta {
    min_zoom: u8,
    max_zoom: u8,
    zoom_and_ifd: HashMap<u8, usize>,
    min_of_samples: Vec<f64>,
    max_of_samples: Vec<f64>,
    zoom_and_tile_across_down: HashMap<u8, (u32, u32)>,
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

    #[allow(clippy::too_many_lines)]
    async fn get_tile(
        &self,
        xyz: TileCoord,
        _url_query: Option<&UrlQuery>,
    ) -> MartinResult<TileData> {
        let tif_file =
            File::open(&self.path).map_err(|e| FileError::IoError(e, self.path.clone()))?;
        let mut decoder =
            Decoder::new(tif_file).map_err(|e| CogError::InvalidTifFile(e, self.path.clone()))?;
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

        let tiles_across = self
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
            })?
            .0;
        let tile_idx = xyz.y * tiles_across + xyz.x;
        let decode_result = decoder
            .read_chunk(tile_idx)
            .map_err(|e| CogError::ReadChunkFailed(e, tile_idx, *ifd, self.path.clone()))?;
        let color_type = decoder
            .colortype()
            .map_err(|e| CogError::InvalidTifFile(e, self.path.clone()))?;

        let tile_width = decoder.chunk_dimensions().0;
        let tile_height = decoder.chunk_dimensions().1;
        let (data_width, data_height) = decoder.chunk_data_dimensions(tile_idx);

        //do more research on the not u8 case, is this the right way to do it?
        let png_file_bytes = match (decode_result, color_type) {
            (DecodingResult::U8(vec), tiff::ColorType::Gray(_)) => to_png(
                vec,
                ColorType::GrayscaleAlpha,
                BitDepth::Eight,
                tile_width,
                tile_height,
                data_width,
                data_height,
                1,
                (true, u8::MAX),
                &self.path,
            ),
            (DecodingResult::U8(vec), tiff::ColorType::RGB(_)) => to_png(
                vec,
                ColorType::Rgba,
                BitDepth::Eight,
                tile_width,
                tile_height,
                data_width,
                data_height,
                3,
                (true, u8::MAX),
                &self.path,
            ),
            (DecodingResult::U8(vec), tiff::ColorType::RGBA(_)) => to_png(
                vec,
                ColorType::Rgba,
                BitDepth::Eight,
                tile_width,
                tile_height,
                data_width,
                data_height,
                4,
                (false, u8::MAX),
                &self.path,
            ),
            (DecodingResult::U16(vec), tiff::ColorType::Gray(_)) => to_png(
                vec,
                ColorType::GrayscaleAlpha,
                BitDepth::Sixteen,
                tile_width,
                tile_height,
                data_width,
                data_height,
                1,
                (true, u16::MAX),
                &self.path,
            ),
            (DecodingResult::U16(vec), tiff::ColorType::RGB(_)) => to_png(
                vec,
                ColorType::Rgba,
                BitDepth::Sixteen,
                tile_width,
                tile_height,
                data_width,
                data_height,
                3,
                (true, u16::MAX),
                &self.path,
            ),
            (DecodingResult::U16(vec), tiff::ColorType::RGBA(_)) => to_png(
                vec,
                ColorType::Rgba,
                BitDepth::Sixteen,
                tile_width,
                tile_height,
                data_width,
                data_height,
                4,
                (false, u16::MAX),
                &self.path,
            ),
            (DecodingResult::U32(vec), tiff::ColorType::Gray(_)) => to_png(
                scale_to_u8(
                    &vec,
                    1,
                    u32::MIN,
                    u32::MAX,
                    &self.meta.min_of_samples,
                    &self.meta.max_of_samples,
                ),
                ColorType::GrayscaleAlpha,
                BitDepth::Eight,
                tile_width,
                tile_height,
                data_width,
                data_height,
                1,
                (true, u8::MAX),
                &self.path,
            ),
            (DecodingResult::F32(vec), tiff::ColorType::Gray(_)) => to_png(
                scale_to_u8(
                    &vec,
                    1,
                    f32::MIN,
                    f32::MAX,
                    &self.meta.min_of_samples,
                    &self.meta.max_of_samples,
                ),
                ColorType::GrayscaleAlpha,
                BitDepth::Eight,
                tile_width,
                tile_height,
                data_width,
                data_height,
                1,
                (true, u8::MAX),
                &self.path,
            ),
            (DecodingResult::F64(vec), tiff::ColorType::Gray(_)) => to_png(
                scale_to_u8(
                    &vec,
                    1,
                    f64::MIN,
                    f64::MAX,
                    &self.meta.min_of_samples,
                    &self.meta.max_of_samples,
                ),
                ColorType::GrayscaleAlpha,
                BitDepth::Eight,
                tile_width,
                tile_height,
                data_width,
                data_height,
                1,
                (true, u8::MAX),
                &self.path,
            ),
            (DecodingResult::I8(vec), tiff::ColorType::Gray(_)) => to_png(
                scale_to_u8(
                    &vec,
                    1,
                    i8::MIN,
                    i8::MAX,
                    &self.meta.min_of_samples,
                    &self.meta.max_of_samples,
                ),
                ColorType::GrayscaleAlpha,
                BitDepth::Eight,
                tile_width,
                tile_height,
                data_width,
                data_height,
                1,
                (true, u8::MAX),
                &self.path,
            ),
            (DecodingResult::I16(vec), tiff::ColorType::Gray(_)) => to_png(
                scale_to_u8(
                    &vec,
                    1,
                    i16::MIN,
                    i16::MAX,
                    &self.meta.min_of_samples,
                    &self.meta.max_of_samples,
                ),
                ColorType::GrayscaleAlpha,
                BitDepth::Eight,
                tile_width,
                tile_height,
                data_width,
                data_height,
                1,
                (true, u8::MAX),
                &self.path,
            ),
            (DecodingResult::I32(vec), tiff::ColorType::Gray(_)) => to_png(
                scale_to_u8(
                    &vec,
                    1,
                    i32::MIN,
                    i32::MAX,
                    &self.meta.min_of_samples,
                    &self.meta.max_of_samples,
                ),
                ColorType::GrayscaleAlpha,
                BitDepth::Eight,
                tile_width,
                tile_height,
                data_width,
                data_height,
                1,
                (true, u8::MAX),
                &self.path,
            ),
            (_, _) => Err(CogError::NotSupportedColorTypeAndBitDepth(
                color_type,
                self.path.clone(),
            )),
        }?;
        Ok(png_file_bytes)
    }
}

#[allow(clippy::cast_sign_loss)]
#[allow(clippy::cast_possible_truncation)]
fn scale_to_u8<T>(
    vec: &[T],
    samples_count: u8,
    min_default: T,
    max_default: T,
    min_values: &[f64],
    max_values: &[f64],
) -> Vec<u8>
where
    T: Copy + NoUninit + PartialOrd + Into<f64>,
{
    vec.iter()
        .enumerate()
        .map(|(i, &value)| {
            let sample_index = i % samples_count as usize;
            let min = min_values
                .get(sample_index)
                .copied()
                .unwrap_or_else(|| min_default.into());
            let max = max_values
                .get(sample_index)
                .copied()
                .unwrap_or_else(|| max_default.into());
            let scaled_value: f64 = (value.into() - min) / (max - min) * 255.0;
            scaled_value.round() as u8
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn to_png<T: Copy + NoUninit + From<u8>>(
    vec: Vec<T>,
    color_type: ColorType,
    bit_depth: BitDepth,
    tile_width: u32,
    tile_height: u32,
    data_width: u32,
    data_height: u32,
    components_count: u32,
    extra_alpha_info: (bool, T),
    path: &Path,
) -> Result<Vec<u8>, CogError> {
    let is_padded = data_width != tile_width;
    let mut buffer = Vec::new();
    {
        let mut encoder = png::Encoder::new(BufWriter::new(&mut buffer), tile_width, tile_height);
        encoder.set_color(color_type);
        encoder.set_depth(bit_depth);

        let mut writer = encoder
            .write_header()
            .map_err(|e| CogError::WritePngHeaderFailed(path.to_path_buf(), e))?;

        let no_data = T::from(0);

        let data: Vec<T> = if let (false, false) = (is_padded, extra_alpha_info.0) {
            vec
        } else {
            let components_count_for_result = if extra_alpha_info.0 {
                components_count + 1
            } else {
                components_count
            };
            let mut result =
                vec![no_data; (tile_width * tile_height * (components_count_for_result)) as usize];
            for row in 0..data_height {
                for col in 0..data_width {
                    let idx_of_chunk = row * data_width * components_count + col * components_count;
                    let idx_of_result = row * tile_width * components_count_for_result
                        + col * components_count_for_result;
                    for component_idx in 0..components_count {
                        result[(idx_of_result + component_idx) as usize] =
                            vec[(idx_of_chunk + component_idx) as usize];
                    }
                    if extra_alpha_info.0 {
                        result[(idx_of_result + components_count) as usize] = extra_alpha_info.1;
                    }
                }
            }
            result
        };

        let u8_vec: &[u8] = bytemuck::cast_slice(&data);
        writer
            .write_image_data(u8_vec)
            .map_err(|e| CogError::WriteToPngFailed(path.to_path_buf(), e))?;
    }
    Ok(buffer)
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
        let tilejson = get_tilejson();
        let tileinfo = TileInfo::new(Format::Png, martin_tile_utils::Encoding::Uncompressed);
        let meta = get_meta(&path)?;
        Ok(Box::new(CogSource {
            id,
            path,
            meta,
            tilejson,
            tileinfo,
        }))
    }

    #[allow(clippy::no_effect_underscore_binding)]
    async fn new_sources_url(&self, _id: String, _url: Url) -> FileResult<Box<dyn Source>> {
        unreachable!()
    }

    fn parse_urls() -> bool {
        false
    }
}

//todo add more to tileJson
fn get_tilejson() -> TileJSON {
    tilejson! {tiles: vec![] }
}

fn get_meta(path: &PathBuf) -> Result<Meta, FileError> {
    let tif_file = File::open(path).map_err(|e| FileError::IoError(e, path.clone()))?;
    let mut decoder = Decoder::new(tif_file)
        .map_err(|e| CogError::InvalidTifFile(e, path.clone()))?
        .with_limits(tiff::decoder::Limits::unlimited());

    let (min_samples, max_samples) = get_minmax_of_samples(&mut decoder, path)?;

    let images_ifd = get_images_ifd(&mut decoder);

    let mut zoom_and_ifd: HashMap<u8, usize> = HashMap::new();
    let mut zoom_and_tile_across_down: HashMap<u8, (u32, u32)> = HashMap::new();

    for image_ifd in &images_ifd {
        decoder
            .seek_to_image(*image_ifd)
            .map_err(|e| CogError::IfdSeekFailed(e, *image_ifd, path.clone()))?;

        let zoom = u8::try_from(images_ifd.len() - (image_ifd + 1))
            .map_err(|_| CogError::TooManyImages(path.clone()))?;

        let planar_configuration: u16 = decoder
            .get_tag_unsigned(Tag::PlanarConfiguration)
            .map_err(|e| {
                CogError::TagsNotFound(
                    e,
                    vec![Tag::PlanarConfiguration.to_u16()],
                    *image_ifd,
                    path.clone(),
                )
            })?;

        if planar_configuration != 1 {
            Err(CogError::PlanaConfigurationNotSupported(
                path.clone(),
                *image_ifd,
                planar_configuration,
            ))?;
        }

        let (tiles_across, tiles_down) = get_across_down(&mut decoder, path, *image_ifd)?;

        zoom_and_ifd.insert(zoom, *image_ifd);
        zoom_and_tile_across_down.insert(zoom, (tiles_across, tiles_down));
    }

    let min_zoom = zoom_and_ifd
        .keys()
        .min()
        .ok_or_else(|| CogError::NoImagesFound(path.clone()))?;

    let max_zoom = zoom_and_ifd
        .keys()
        .max()
        .ok_or_else(|| CogError::NoImagesFound(path.clone()))?;
    Ok(Meta {
        min_zoom: *min_zoom,
        max_zoom: *max_zoom,
        zoom_and_ifd,
        min_of_samples: min_samples,
        max_of_samples: max_samples,
        zoom_and_tile_across_down,
    })
}

fn get_minmax_of_samples(
    decoder: &mut Decoder<File>,
    path: &Path,
) -> Result<(Vec<f64>, Vec<f64>), CogError> {
    let gdal_metadata_tag = Tag::Unknown(42112);
    let metadata = decoder.get_tag_ascii_string(gdal_metadata_tag);

    let mut min_values = Vec::new();
    let mut max_values = Vec::new();

    if let Ok(metadata_text) = metadata {
        if let Ok(re_min) =
            Regex::new(r#"<Item name="STATISTICS_MINIMUM" sample="(\d+)">([\d.]+)</Item>"#)
        {
            for cap in re_min.captures_iter(&metadata_text) {
                let value: f64 = cap[2].parse::<f64>().map_err(|_| {
                    CogError::ParseSTATISTICSValueFailed(
                        "STATISTICS_MINIMUM".to_string(),
                        path.to_path_buf(),
                    )
                })?;
                min_values.push(value);
            }
        } else {
            //todo log
        }

        if let Ok(re_max) =
            Regex::new(r#"<Item name="STATISTICS_MAXIMUM" sample="(\d+)">([\d.]+)</Item>"#)
        {
            for cap in re_max.captures_iter(&metadata_text) {
                let value: f64 = cap[2].parse().map_err(|_| {
                    CogError::ParseSTATISTICSValueFailed(
                        "sample of STATISTICS_MINIMUM".to_string(),
                        path.to_path_buf(),
                    )
                })?;
                max_values.push(value);
            }
        } else {
            //todo log
        }
    }

    Ok((min_values, max_values))
}

fn get_across_down(
    decoder: &mut Decoder<File>,
    path: &Path,
    image_ifd: usize,
) -> Result<(u32, u32), FileError> {
    let (tile_width, tile_height) = (decoder.chunk_dimensions().0, decoder.chunk_dimensions().1);
    let (image_width, image_length) = get_image_width_length(decoder, path, image_ifd)?;
    let tiles_across = (image_width + tile_width - 1) / tile_width;
    let tiles_down = (image_length + tile_height - 1) / tile_height;

    Ok((tiles_across, tiles_down))
}

fn get_image_width_length(
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

fn get_images_ifd(decoder: &mut Decoder<File>) -> Vec<usize> {
    let mut res = vec![];
    let mut ifd_idx = 0;
    loop {
        let is_image = decoder
            .get_tag_u32(Tag::NewSubfileType) //based on the tiff6.0 spec, it's 32-bit(4-byte)unsigned integer
            .map_or_else(|_| true, |v| v & 4 != 4);
        if is_image {
            //todo We should not ignore mask in the future
            res.push(ifd_idx);
        }

        ifd_idx += 1;

        let next_res = decoder.seek_to_image(ifd_idx);
        if next_res.is_err() {
            break;
        }
    }
    res
}

// #[cfg(test)]
// mod tests {
//     use std::{fs::File, io::Write, path::PathBuf};

//     use martin_tile_utils::{TileCoord, TileInfo};
//     use tilejson::tilejson;

//     use crate::Source;

//     use super::get_meta;

//     #[actix_rt::test]
//     async fn can_get_tile() -> () {
//         let path = PathBuf::from("../tests/fixtures/cog//rgb_u8.tif");
//         let meta = get_meta(&path).unwrap();
//         let source = super::CogSource {
//             id: "test".to_string(),
//             path,
//             meta,
//             tilejson: tilejson! {tiles: vec![] },
//             tileinfo: TileInfo {
//                 format: martin_tile_utils::Format::Png,
//                 encoding: martin_tile_utils::Encoding::Uncompressed,
//             },
//         };
//         let query = None;
//         let _tile = source.get_tile(TileCoord { z: 0, x: 0, y: 0 }, query).await;
//         let _bytes = _tile.unwrap();
//         //write this bytes to a "result.png" file

//         let mut file = File::create("result.png").unwrap();
//         file.write_all(&_bytes).unwrap();

//         todo!()
//     }
// }

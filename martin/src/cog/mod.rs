mod errors;

pub use errors::CogError;

use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::vec;
use std::{fmt::Debug, path::PathBuf};

use std::io::BufWriter;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag::{self, GdalNodata};

use async_trait::async_trait;
use martin_tile_utils::{Format, TileCoord, TileInfo};
use serde::{Deserialize, Serialize};
use tilejson::{tilejson, TileJSON};
use url::Url;

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
    zoom_and_tile_across_down: HashMap<u8, (u32, u32)>,
    nodata: Option<f64>,
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
    result_dims: (u32, u32),
    chunk_dims: (u32, u32),
    chunk_components_count: u32,
    nodata: Option<u8>,
    path: &Path,
) -> Result<Vec<u8>, CogError> {
    let (data_width, data_height) = chunk_dims;
    let (tile_width, tile_height) = result_dims;
    let is_padded = data_width != tile_width;
    let need_add_alpha = chunk_components_count != 4;
    let default_value = 0;

    let pixels = if nodata.is_some() || need_add_alpha || is_padded {
        let mut result_vec = vec![default_value; (tile_width * tile_height * 4) as usize];
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

    let color_type = decoder
        .colortype()
        .map_err(|e| CogError::InvalidTifFile(e, path.clone()))?;

    if !matches!(
        color_type,
        tiff::ColorType::RGB(8) | tiff::ColorType::RGBA(8)
    ) {
        Err(CogError::NotSupportedColorTypeAndBitDepth(
            color_type,
            path.clone(),
        ))?;
    }

    decoder
        .get_tag_unsigned(Tag::PlanarConfiguration)
        .map_err(|e| {
            CogError::TagsNotFound(e, vec![Tag::PlanarConfiguration.to_u16()], 0, path.clone())
        })
        .and_then(|config| {
            if config == 1 {
                Ok(())
            } else {
                Err(CogError::PlanaConfigurationNotSupported(
                    path.clone(),
                    0,
                    config,
                ))
            }
        })?;

    let tag = decoder.get_tag_ascii_string(GdalNodata);
    let nodata: Option<f64> = if let Ok(nodata_tag) = tag {
        nodata_tag.parse().ok()
    } else {
        None
    };
    let images_ifd = get_images_ifd(&mut decoder);

    let mut zoom_and_ifd: HashMap<u8, usize> = HashMap::new();
    let mut zoom_and_tile_across_down: HashMap<u8, (u32, u32)> = HashMap::new();

    for image_ifd in &images_ifd {
        decoder
            .seek_to_image(*image_ifd)
            .map_err(|e| CogError::IfdSeekFailed(e, *image_ifd, path.clone()))?;

        let zoom = u8::try_from(images_ifd.len() - (image_ifd + 1))
            .map_err(|_| CogError::TooManyImages(path.clone()))?;

        let (tiles_across, tiles_down) = get_grid_dims(&mut decoder, path, *image_ifd)?;

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

fn get_images_ifd(decoder: &mut Decoder<File>) -> Vec<usize> {
    let mut res = vec![];
    let mut ifd_idx = 0;
    loop {
        let is_image = decoder
            .get_tag_u32(Tag::NewSubfileType)
            .map_or_else(|_| true, |v| v & 4 != 4);
        if is_image {
            //todo We should not ignore mask in the next PRs
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

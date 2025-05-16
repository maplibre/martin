use std::{fs::File, path::Path};

use tiff::{decoder::Decoder, tags::Tag};

use super::CogError;

// See https://docs.ogc.org/is/19-008r4/19-008r4.html#_coordinate_transformations
//         ModelPixelScaleTag
//          ModelTiepointTag
//  R ------------ OR --------------> M
// (I,J,K) ModelTransformationTag (X,Y,Z)
#[derive(Clone, Debug)]
pub struct ModelInfo {
    pub pixel_scale: Option<Vec<f64>>,
    pub tie_points: Option<Vec<f64>>,
    pub transformation: Option<Vec<f64>>,
}

pub fn get_model_infos(decoder: &mut Decoder<File>, path: &Path) -> ModelInfo {
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
    ModelInfo {
        pixel_scale,
        tie_points,
        transformation,
    }
}

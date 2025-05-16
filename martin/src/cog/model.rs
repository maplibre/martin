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
    /*
       ModelPixelScaleTag:
       Tag = 33550 (830E.H)
       Type = DOUBLE (IEEE Double precision)
       N = 3

       This tag may be used to specify the size of raster pixel spacing in the model space units, when the raster space can be embedded in the model space coordinate reference system without rotation, and consists of the following 3 values: (ScaleX, ScaleY, ScaleZ)
       Example: (10.0, 10.0, 0.0)
       see https://docs.ogc.org/is/19-008r4/19-008r4.html#_geotiff_tags_for_coordinate_transformations
    */
    pub pixel_scale: Option<Vec<f64>>,
    /*
       ModelTiepointTag:
       Tag = 33922 (8482.H)
       Type = DOUBLE (IEEE Double precision)
       N = 6*K, K = number of tiepoints
       Alias: GeoreferenceTag
       Example: (0, 0, 0, 350807.4, 5316081.3, 0.0)

       This tag stores raster→model tiepoint pairs in the order: ModelTiepointTag = (...,I,J,K, X,Y,Z...),where (I,J,K) is the point at location (I,J) in raster space with pixel-value K, and (X,Y,Z) is a vector in model space. In most cases the model space is only two-dimensional, in which case both K and Z should be set to zero; this third dimension is provided in anticipation of support for 3D digital elevation models and vertical coordinate systems.
       see https://docs.ogc.org/is/19-008r4/19-008r4.html#_geotiff_tags_for_coordinate_transformations
    */
    pub tie_points: Option<Vec<f64>>,
    /*
       ModelTransformationTag
       Tag = 34264 (85D8.H)
       Type = DOUBLE
       N = 16

       ModelTransformationTag = (a,b,c,d,e....m,n,o,p).

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

       This matrix tag should not be used if the ModelTiepointTag and the ModelPixelScaleTag are already defined.
    */
    pub transformation: Option<Vec<f64>>,
}

pub fn get_model_infos(decoder: &mut Decoder<File>, path: &Path) -> ModelInfo {
    let pixel_scale = decoder
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

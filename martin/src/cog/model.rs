use std::{fs::File, path::Path};

use tiff::{decoder::Decoder, tags::Tag};

use super::CogError;

/// These are tags to be used for defining the relationship between raster space and model space. See [ogc doc](https://docs.ogc.org/is/19-008r4/19-008r4.html#_coordinate_transformations) for more details.
///
/// The the relationship may be diagrammed as:
/// ```raw
///        ModelPixelScaleTag
///          ModelTiepointTag
///  R ------------ OR --------------> M
/// (I,J,K) ModelTransformationTag (X,Y,Z)
/// ```
#[derive(Clone, Debug)]
pub struct ModelInfo {
    /// `ModelPixelScaleTag`, may be used to specify the size of raster pixel spacing in the model space units, when the raster space can be embedded in the model space coordinate reference system without rotation.
    /// Consists of the following 3 values: `(ScaleX, ScaleY, ScaleZ)`.
    ///    
    /// ```raw
    /// ModelPixelScaleTag:
    ///   Tag = 33550 (830E.H)
    ///   Type = DOUBLE (IEEE Double precision)
    ///   N = 3
    /// ```
    ///
    /// Example: `[10.0, 10.0, 0.0]`
    pub pixel_scale: Option<Vec<f64>>,
    /// This tag stores raster→model tiepoint pairs.
    ///
    /// Ordering among the points is `ModelTiepointTag = (...,I,J,K, X,Y,Z...)`, where `I,J,K` is the point at location `I,J` in raster space with pixel-value `K`, and `X,Y,Z` is a vector in model space.
    ///
    /// ```raw
    /// ModelTiepointTag:
    ///   Tag = 33922 (8482.H)
    ///   Type = DOUBLE (IEEE Double precision)
    ///   N = 6*K, K = number of tiepoints
    ///   Alias: GeoreferenceTag
    /// ```
    ///
    /// Example: `[0, 0, 0, 350807.4, 5316081.3, 0.0]`
    pub tie_points: Option<Vec<f64>>,
    /// This tag may be used to specify the transformation matrix between the raster space (and its dependent pixel-value space) and the (possibly 3D) model space.
    ///
    /// ```raw
    /// ModelTransformationTag:
    ///   Tag = 34264 (85D8.H)
    ///   Type = DOUBLE
    ///   N = 16
    /// ```
    ///
    /// If specified, the tag has the following organization: `ModelTransformationTag` = (a,b,c,d,e....m,n,o,p) where
    /// model                  image
    /// coords =     matrix  * coords
    /// |- -|     |-       -|  |- -|
    /// | X |     | a b c d |  | I |
    /// | | |     |         |  |   |
    /// | Y |     | e f g h |  | J |
    /// |   |  =  |         |  |   |
    /// | Z |     | i j k l |  | K |
    /// | | |     |         |  |   |
    /// | 1 |     | m n o p |  | 1 |
    /// |- -|     |-       -|  |- -|
    pub transformation: Option<Vec<f64>>,
}

impl ModelInfo {
    pub fn decode(decoder: &mut Decoder<File>, path: &Path) -> ModelInfo {
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
}

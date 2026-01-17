use std::fs::File;
use std::path::Path;

use tiff::decoder::Decoder;
use tiff::tags::Tag;

use crate::tiles::cog::CogError;

/// These tags define the relationship between raster space and model space.
/// See [ogc doc](https://docs.ogc.org/is/19-008r4/19-008r4.html#_coordinate_transformations) for details.
///
/// The relationship may be diagrammed as:
/// ```raw
///        ModelPixelScaleTag
///          ModelTiepointTag
///  R ------------ OR --------------> M
/// (I,J,K) ModelTransformationTag (X,Y,Z)
/// ```
#[derive(Clone, Debug)]
pub struct ModelInfo {
    /// `ModelPixelScaleTag` may be used to specify the size of raster pixel spacing in the model space units, when the raster space can be embedded in the model space coordinate reference system without rotation.
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
    ///   N = 6*K, K = number of tie-points
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
    pub projected_crs: Option<u16>,
}

impl ModelInfo {
    /// Extracts `GeoTIFF` model information from TIFF decoder.
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

        let mut projected_crs: Option<u16> = None;
        // See: https://docs.ogc.org/is/19-008r4/19-008r4.html#_requirements_class_geokeydirectorytag
        if let Ok(geokeys) = decoder.get_tag_u16_vec(Tag::GeoKeyDirectoryTag) {
            let mut i = 0;
            for chunk in geokeys.chunks_exact(4) {
                if i == 0 {
                    if !chunk
                        .get(0)
                        .is_some_and(|key_directory_version| *key_directory_version == 1u16)
                    {
                        break;
                    }
                    if !chunk
                        .get(1)
                        .is_some_and(|key_revision| *key_revision == 1u16)
                    {
                        break;
                    }
                    if !chunk
                        .get(2)
                        .is_some_and(|minor_revision| *minor_revision == 0u16)
                    {
                        break;
                    }
                    if !chunk.get(3).is_some_and(|n_keys| *n_keys > 0u16) {
                        break;
                    }
                } else {
                    // See: https://docs.ogc.org/is/19-008r4/19-008r4.html#_requirements_class_projectedcrsgeokey
                    if !chunk.get(0).is_some_and(|key_id| *key_id == 3072u16) {
                        continue;
                    }
                    projected_crs = chunk.get(3).map(|v| *v);
                }
                i += 1;
            }
        }

        ModelInfo {
            pixel_scale,
            tie_points,
            transformation,
            projected_crs,
        }
    }
}

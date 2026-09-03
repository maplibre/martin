//! Encodes traced contour features into Mapbox Vector Tile bytes.
//!
//! Contours are always linestrings, so this has no polygon-ring handling: a
//! traced ring is emitted open and the client closes nothing.

use std::num::NonZeroU32;

use geo::MapCoords as _;
use geo_types::{Coord, Geometry};
use martin_tile_utils::TileData;
use mlt_core::fast_mvt::{MvtTileBuilder, MvtValue};

use super::error::ContourError;
use super::features::GeometryFeatures;

/// Where encoded contour features are written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MvtEncodingOptions {
    /// Name of the single layer the features go into.
    pub layer_name: String,
    /// Tile extent advertised on that layer.
    pub extent: u32,
}

impl Default for MvtEncodingOptions {
    fn default() -> Self {
        Self {
            layer_name: "contour".to_owned(),
            extent: 4096,
        }
    }
}

/// Encodes tile-space contour features into a single-layer MVT tile.
///
/// Coordinates are rounded to the integer tile grid the encoder consumes;
/// lines that collapse to fewer than two distinct grid vertices are dropped,
/// since an MVT linestring needs a `MoveTo` and at least one `LineTo`.
///
/// # Errors
///
/// Returns [`ContourError::Encoding`] when the layer name is invalid or the
/// encoder rejects a feature or tag.
pub fn encode_contours(
    features: &GeometryFeatures,
    opts: &MvtEncodingOptions,
) -> Result<TileData, ContourError> {
    let extent = NonZeroU32::new(opts.extent)
        .ok_or_else(|| ContourError::Encoding("an MVT extent must not be zero".to_owned()))?;

    let mut layer = MvtTileBuilder::with_capacity(1)
        .layer_with_capacity(&opts.layer_name, features.len())
        .map_err(|e| ContourError::Encoding(e.to_string()))?;
    layer.extent(extent);

    for contour in features {
        let Some(geometry) = to_tile_geometry(&contour.geometry) else {
            continue;
        };
        let mut feature = layer
            .feature(&geometry)
            .map_err(|e| ContourError::Encoding(e.to_string()))?;
        feature
            .tag("ele", MvtValue::auto_int(contour.elevation))
            .and_then(|f| f.tag("major", MvtValue::Bool(contour.is_major)))
            .map_err(|e| ContourError::Encoding(e.to_string()))?;
        layer = feature.end();
    }

    Ok(layer.end().encode())
}

/// Rounds a tile-space line onto the integer grid the encoder consumes,
/// dropping consecutive duplicate vertices.
///
/// Returns `None` when fewer than two distinct vertices survive.
fn to_tile_geometry(line: &geo_types::LineString<f64>) -> Option<mlt_core::fast_mvt::MvtGeometry> {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "tile-space coordinates stay within roughly 0..=extent plus the apron"
    )]
    let rounded = line.map_coords(|c| Coord {
        x: c.x.round() as i32,
        y: c.y.round() as i32,
    });

    let mut vertices: Vec<Coord<i32>> = Vec::with_capacity(rounded.0.len());
    for vertex in rounded.0 {
        if vertices.last() != Some(&vertex) {
            vertices.push(vertex);
        }
    }
    if vertices.len() < 2 {
        return None;
    }
    Some(Geometry::LineString(geo_types::LineString(vertices)))
}

#[cfg(test)]
mod tests {

    use mlt_core::fast_mvt::{MvtLayerRef, MvtReaderRef, MvtValueRef};

    use super::super::features::ContourFeature;
    use super::super::isolines::DEFAULT_MVT_EXTENT;
    use super::*;

    fn feature(points: &[(f64, f64)], elevation: i16, is_major: bool) -> ContourFeature {
        ContourFeature {
            geometry: geo_types::LineString::from(points.to_vec()),
            elevation,
            is_major,
        }
    }

    /// Features the encoder actually emitted into the contour layer.
    fn encoded_feature_count(features: Vec<ContourFeature>) -> usize {
        let opts = MvtEncodingOptions::default();
        let bytes = encode_contours(&GeometryFeatures::new(features), &opts)
            .expect("encoding should succeed");
        let reader = MvtReaderRef::new(&bytes).expect("tile should parse");
        reader
            .layers()
            .filter(|layer| layer.name() == opts.layer_name)
            .map(MvtLayerRef::feature_count)
            .sum()
    }

    #[test]
    fn a_real_line_is_kept() {
        let line = feature(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)], 300, false);
        assert_eq!(encoded_feature_count(vec![line]), 1);
    }

    #[test]
    fn line_collapsing_to_a_single_grid_vertex_is_dropped() {
        // Every vertex rounds onto (10, 10): a sub-pixel line the encoder would
        // emit as a bare MoveTo.
        let degenerate = feature(&[(10.1, 10.1), (10.3, 10.1), (10.1, 10.3)], 300, false);
        assert_eq!(encoded_feature_count(vec![degenerate]), 0);
    }

    #[test]
    fn degenerate_lines_are_dropped_without_taking_real_ones_with_them() {
        let valid = feature(&[(0.0, 0.0), (100.0, 100.0)], 300, false);
        let degenerate = feature(&[(10.1, 10.1), (10.3, 10.1)], 300, false);
        assert_eq!(encoded_feature_count(vec![valid, degenerate]), 1);
    }

    #[test]
    fn an_all_degenerate_tile_encodes_no_features() {
        let degenerate = feature(&[(5.2, 5.2), (5.4, 5.4)], 300, false);
        assert_eq!(encoded_feature_count(vec![degenerate]), 0);
    }

    #[test]
    fn tags_round_trip_as_an_int_and_a_bool() {
        let opts = MvtEncodingOptions::default();
        let features =
            GeometryFeatures::new(vec![feature(&[(0.0, 0.0), (100.0, 100.0)], 1234, true)]);
        let bytes = encode_contours(&features, &opts).expect("encoding should succeed");

        let reader = MvtReaderRef::new(&bytes).expect("tile should parse");
        let layer = reader
            .layers()
            .find(|layer| layer.name() == opts.layer_name)
            .expect("the contour layer exists");
        assert_eq!(layer.extent(), DEFAULT_MVT_EXTENT);

        let feature = layer.features().next().expect("one feature");
        let tags = feature.properties_vec().expect("properties should read");

        let elevation = tags
            .iter()
            .find(|(key, _)| *key == "ele")
            .map(|(_, value)| *value)
            .expect("the elevation tag is present");
        assert!(
            matches!(elevation, MvtValueRef::UInt(1234) | MvtValueRef::SInt(1234)),
            "elevation should encode as an integer: {elevation:?}"
        );

        let major = tags
            .iter()
            .find(|(key, _)| *key == "major")
            .map(|(_, value)| *value)
            .expect("the major tag is present");
        assert!(
            matches!(major, MvtValueRef::Bool(true)),
            "major should encode as a bool: {major:?}"
        );
    }

    #[test]
    fn a_zero_extent_is_rejected() {
        let opts = MvtEncodingOptions {
            extent: 0,
            ..Default::default()
        };
        let result = encode_contours(&GeometryFeatures::default(), &opts);
        assert!(matches!(result, Err(ContourError::Encoding(_))));
    }
}

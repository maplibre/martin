use fast_mvt::{MvtFeatureBuilder, MvtValue};
use geo::{BoundingRect as _, MapCoords as _};
use geo_index::rtree::sort::HilbertSort;
use geo_index::rtree::{RTree, RTreeBuilder};
use geojson::{GeoJson, JsonValue};
use martin_tile_utils::{EARTH_CIRCUMFERENCE, wgs84_to_webmercator};
use serde_json::Map;

use crate::tiles::geojson::error::GeoJsonError;

/// A feature ready to be served: a geometry plus its `GeoJSON` properties.
///
/// The coordinate type `T` tracks which space the geometry lives in: `f64` for the preprocessed
/// Web Mercator (EPSG:3857) features, and `i32` once clipped and snapped to the MVT tile grid.
#[derive(Clone)]
pub(crate) struct PreparedFeature<T: geo_types::CoordNum = f64> {
    /// Feature geometry.
    pub(crate) geom: geo_types::Geometry<T>,
    /// `GeoJSON` feature properties, carried verbatim into the MVT feature.
    pub(crate) properties: Option<Map<String, JsonValue>>,
}

/// Features ready to serve, their spatial index, and the data bounding box in Web Mercator
/// (`None` when no feature contributed a geometry).
type Preprocessed = (
    Vec<PreparedFeature>,
    RTree<f64>,
    Option<geo_types::Rect<f64>>,
);

/// Preprocess a parsed `GeoJSON` document into features ready to serve.
///
/// 1. Keep only features that carry a geometry.
/// 2. Reproject geometries from WGS84 to Web Mercator.
/// 3. Index every geometry's bounding box in a packed Hilbert R-tree.
pub(crate) fn preprocess_geojson(geojson: GeoJson) -> Result<Preprocessed, GeoJsonError> {
    let raw = match geojson {
        GeoJson::FeatureCollection(fc) => fc
            .features
            .into_iter()
            .filter_map(|f| Some((f.geometry?, f.properties)))
            .collect::<Vec<_>>(),
        GeoJson::Feature(f) => f.geometry.map(|g| (g, f.properties)).into_iter().collect(),
        GeoJson::Geometry(g) => vec![(g, None)],
    };

    let mut features = Vec::with_capacity(raw.len());
    let mut bboxes = Vec::with_capacity(raw.len());
    for (geometry, properties) in raw {
        let geom = geo_types::Geometry::<f64>::try_from(geometry.value)
            .map_err(|e| GeoJsonError::GeoJsonError(Box::new(e)))?;
        let geom = geom.map_coords(|c| {
            let (x, y) = wgs84_to_webmercator(c.x, c.y);
            geo_types::Coord { x, y }
        });
        // An empty geometry has no extent to index or serve.
        let Some(bbox) = geom.bounding_rect() else {
            continue;
        };
        let (min, max) = (bbox.min(), bbox.max());
        bboxes.push([min.x, min.y, max.x, max.y]);
        features.push(PreparedFeature { geom, properties });
    }

    let feature_count =
        u32::try_from(features.len()).map_err(|_| GeoJsonError::TooManyFeatures(features.len()))?;
    let mut builder = RTreeBuilder::<f64>::new(feature_count);
    for bbox in &bboxes {
        builder.add(bbox[0], bbox[1], bbox[2], bbox[3]);
    }
    let tree = builder.finish::<HilbertSort>();

    // The data bounding box is the union of every feature's bbox.
    let data_bounds = bboxes
        .iter()
        .copied()
        .reduce(|a, b| {
            [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]
        })
        .map(|[min_x, min_y, max_x, max_y]| {
            geo_types::Rect::new(
                geo_types::Coord { x: min_x, y: min_y },
                geo_types::Coord { x: max_x, y: max_y },
            )
        });

    Ok((features, tree, data_bounds))
}

pub(crate) fn tile_length_from_zoom(zoom: u8) -> f64 {
    EARTH_CIRCUMFERENCE / f64::from(1_u32 << zoom)
}

/// Copy `GeoJSON` properties onto an MVT feature as attribute tags.
/// Geometries carry no attributes once converted to `geo_types`, so this is the only feature
/// metadata copied through to the encoder. Null-valued properties are omitted; arrays and objects
/// are serialized to a JSON string, matching the MVT value model which has no composite types.
pub(crate) fn add_properties(
    feature: &mut MvtFeatureBuilder,
    properties: Map<String, JsonValue>,
) -> Result<(), GeoJsonError> {
    for (key, value) in properties {
        let mvt_value = match value {
            // MVT has no composite value type, so arrays and objects are serialized to a JSON string.
            JsonValue::Array(_) | JsonValue::Object(_) => match serde_json::to_string(&value) {
                Ok(v) => MvtValue::String(v),
                Err(_) => return Err(GeoJsonError::UnsupportedProperty(key)),
            },
            // Scalars (and null) convert straight through fast-mvt's `serde_json::Value` mapping.
            // Non-finite or arbitrary-precision numbers have no MVT representation and error out.
            _ => match MvtValue::try_from(value) {
                Ok(v) => v,
                Err(_) => return Err(GeoJsonError::UnsupportedProperty(key)),
            },
        };
        // A null property yields `MvtValue::Null`, which `tag` skips.
        feature
            .tag(key, mvt_value)
            .map_err(GeoJsonError::MvtError)?;
    }
    Ok(())
}

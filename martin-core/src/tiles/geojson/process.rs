use geo::{BoundingRect as _, MapCoords as _};
use geo_index::rtree::sort::HilbertSort;
use geo_index::rtree::{RTree, RTreeBuilder};
use geojson::{GeoJson, JsonValue};
use geozero::error::GeozeroError;
use geozero::{ColumnValue, PropertyProcessor};
use martin_tile_utils::{EARTH_CIRCUMFERENCE, wgs84_to_webmercator};
use serde_json::Map;

use crate::tiles::geojson::error::GeoJsonError;
use crate::tiles::geojson::rect::Rect;

/// A feature ready to be served: a Web Mercator geometry plus its `GeoJSON` properties.
#[derive(Clone)]
pub(crate) struct PreparedFeature {
    /// Geometry in Web Mercator (EPSG:3857).
    pub(crate) geom: geo_types::Geometry<f64>,
    /// `GeoJSON` feature properties, carried verbatim into the MVT feature.
    pub(crate) properties: Option<Map<String, JsonValue>>,
}

/// Features ready to serve, their spatial index, and the data bounding box in Web Mercator
/// (`None` when no feature contributed a geometry).
type Preprocessed = (Vec<PreparedFeature>, RTree<f64>, Option<Rect>);

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
    let mut data_bounds = Rect::default();
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
        data_bounds.extend(&[min.x, min.y]);
        data_bounds.extend(&[max.x, max.y]);
        bboxes.push([min.x, min.y, max.x, max.y]);
        features.push(PreparedFeature { geom, properties });
    }

    let feature_count = u32::try_from(features.len())
        .map_err(|_| GeoJsonError::TooManyFeatures(features.len()))?;
    let mut builder = RTreeBuilder::<f64>::new(feature_count);
    for bbox in &bboxes {
        builder.add(bbox[0], bbox[1], bbox[2], bbox[3]);
    }
    let tree = builder.finish::<HilbertSort>();

    Ok((features, tree, data_bounds.into_finite()))
}

pub(crate) fn tile_length_from_zoom(zoom: u8) -> f64 {
    EARTH_CIRCUMFERENCE / f64::from(1_u32 << zoom)
}

/// Process `GeoJSON` properties into MVT feature attributes.
/// Geometries carry no attributes once converted to `geo_types`, so this is the only feature
/// metadata copied through to the encoder.
pub(crate) fn process_properties<P: PropertyProcessor>(
    properties: &Map<String, JsonValue>,
    processor: &mut P,
) -> Result<(), GeozeroError> {
    for (i, (key, value)) in properties.iter().enumerate() {
        // Could we provide a stable property index?
        match value {
            JsonValue::String(v) => processor.property(i, key, &ColumnValue::String(v))?,
            JsonValue::Number(v) => {
                if let Some(n) = v.as_i64() {
                    processor.property(i, key, &ColumnValue::Long(n))?
                } else if let Some(n) = v.as_u64() {
                    processor.property(i, key, &ColumnValue::ULong(n))?
                } else if let Some(n) = v.as_f64() {
                    processor.property(i, key, &ColumnValue::Double(n))?
                } else {
                    // Non-finite or arbitrary-precision numbers cannot be represented as an MVT value
                    return Err(GeozeroError::Property(key.clone()));
                }
            }
            JsonValue::Bool(v) => processor.property(i, key, &ColumnValue::Bool(*v))?,
            JsonValue::Array(v) => {
                let json_string =
                    serde_json::to_string(v).map_err(|_err| GeozeroError::Property(key.clone()))?;
                processor.property(i, key, &ColumnValue::Json(&json_string))?
            }
            JsonValue::Object(v) => {
                let json_string =
                    serde_json::to_string(v).map_err(|_err| GeozeroError::Property(key.clone()))?;
                processor.property(i, key, &ColumnValue::Json(&json_string))?
            }
            // For null values omit the property
            JsonValue::Null => false,
        };
    }
    Ok(())
}

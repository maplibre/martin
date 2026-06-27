use std::collections::BTreeMap;

use tilejson::{Bounds, TileJSON, VectorLayer, tilejson};

use crate::config::file::tiles::duckdb::resolver::geoparquet::introspect::GeoParquetIntrospection;
use crate::config::file::tiles::duckdb::sources::GeoParquetEntry;

pub(crate) fn build_tilejson(
    introspection: &GeoParquetIntrospection,
    entry: &GeoParquetEntry,
    source_id: &str,
    source_label: &str,
    bounds: Option<Bounds>,
) -> TileJSON {
    let layer_id = entry
        .layer_id
        .clone()
        .unwrap_or_else(|| source_id.to_string());

    let layer = VectorLayer {
        id: layer_id,
        fields: introspection.property_columns.clone(),
        description: None,
        maxzoom: None,
        minzoom: None,
        other: BTreeMap::default(),
    };

    tilejson! {
        tiles: vec![],
        vector_layers: vec![layer],
        name: source_id.to_string(),
        description: format!("GeoParquet ({source_label})"),
        minzoom: entry.minzoom.unwrap_or(0),
        maxzoom: entry.maxzoom.unwrap_or(20),
        bounds: bounds.unwrap_or_default(),
    }
}

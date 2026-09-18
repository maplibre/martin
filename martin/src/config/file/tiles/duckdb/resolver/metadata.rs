use std::collections::BTreeMap;

use tilejson::{Bounds, TileJSON, VectorLayer, tilejson};

use crate::config::file::tiles::duckdb::resolver::introspect::LayerIntrospection;
use crate::config::file::tiles::duckdb::sources::MvtLayerOptions;

pub(crate) fn build_tilejson(
    introspection: &LayerIntrospection,
    layer: &MvtLayerOptions,
    layer_id: &str,
    source_id: &str,
    description: String,
    bounds: Option<Bounds>,
) -> TileJSON {
    let vector_layer = VectorLayer {
        id: layer_id.to_owned(),
        fields: introspection.property_columns.clone(),
        description: None,
        maxzoom: None,
        minzoom: None,
        other: BTreeMap::default(),
    };

    let mut tilejson = tilejson! {
        tiles: vec![],
        vector_layers: vec![vector_layer],
        name: source_id.to_owned(),
        description: description,
    };
    tilejson.minzoom = layer.minzoom;
    tilejson.maxzoom = layer.maxzoom;
    tilejson.bounds = bounds;
    tilejson
}

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tilejson::{Bounds, TileJSON, VectorLayer};

use crate::config::UnrecognizedValues;
use crate::pg::config::PgInfo;
use crate::pg::utils::{patch_json, InfoMap};
use crate::utils::sorted_opt_map;

pub type TableInfoSources = InfoMap<TableInfo>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct TableInfo {
    /// ID of the layer as specified in a tile (ST_AsMVT param)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer_id: Option<String>,

    /// Table schema
    pub schema: String,

    /// Table name
    pub table: String,

    /// Geometry SRID
    pub srid: i32,

    /// Geometry column name
    pub geometry_column: String,

    /// Geometry column has a spatial index
    #[serde(skip_deserializing, skip_serializing)]
    pub geometry_index: Option<bool>,

    /// Flag indicating if table is actually a view (PostgreSQL relkind = 'v')
    #[serde(skip_deserializing, skip_serializing)]
    pub is_view: Option<bool>,

    /// Feature id column name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_column: Option<String>,

    /// An integer specifying the minimum zoom level
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minzoom: Option<u8>,

    /// An integer specifying the maximum zoom level. MUST be >= minzoom
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maxzoom: Option<u8>,

    /// The maximum extent of available map tiles. Bounds MUST define an area
    /// covered by all zoom levels. The bounds are represented in WGS:84
    /// latitude and longitude values, in the order left, bottom, right, top.
    /// Values may be integers or floating point numbers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,

    /// Tile extent in tile coordinate space
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extent: Option<u32>,

    /// Buffer distance in tile coordinate space to optionally clip geometries
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer: Option<u32>,

    /// Boolean to control if geometries should be clipped or encoded as is
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip_geom: Option<bool>,

    /// Geometry type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geometry_type: Option<String>,

    /// List of columns, that should be encoded as tile properties
    #[serde(serialize_with = "sorted_opt_map")]
    pub properties: Option<HashMap<String, String>>,

    /// Mapping of properties to the actual table columns
    #[serde(skip_deserializing, skip_serializing)]
    pub prop_mapping: HashMap<String, String>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,

    /// TileJSON provider by the SQL comment. Shouldn't be serialized
    #[serde(skip)]
    pub tilejson: Option<serde_json::Value>,
}

impl PgInfo for TableInfo {
    fn format_id(&self) -> String {
        format!("{}.{}.{}", self.schema, self.table, self.geometry_column)
    }

    fn to_tilejson(&self, source_id: String) -> TileJSON {
        let mut tilejson = tilejson::tilejson! {
            tiles: vec![],  // tile source is required, but not yet known
            name: source_id.clone(),
            description: self.format_id(),
        };
        tilejson.minzoom = self.minzoom;
        tilejson.maxzoom = self.maxzoom;
        tilejson.bounds = self.bounds;
        let layer = VectorLayer {
            id: source_id,
            fields: self.properties.clone().unwrap_or_default(),
            description: None,
            maxzoom: None,
            minzoom: None,
            other: HashMap::default(),
        };
        tilejson.vector_layers = Some(vec![layer]);
        patch_json(tilejson, &self.tilejson)
    }
}

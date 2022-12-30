use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use tilejson::{Bounds, TileJSON};

use crate::pg::config::PgInfo;
use crate::utils::{create_tilejson, InfoMap};

pub type TableInfoSources = InfoMap<TableInfo>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct TableInfo {
    /// Table schema
    pub schema: String,

    /// Table name
    pub table: String,

    /// Geometry SRID
    pub srid: i32,

    /// Geometry column name
    pub geometry_column: String,

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
    pub properties: HashMap<String, String>,

    /// Mapping of properties to the actual table columns
    #[serde(skip_deserializing, skip_serializing)]
    pub prop_mapping: HashMap<String, String>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: HashMap<String, Value>,
}

impl PgInfo for TableInfo {
    fn format_id(&self) -> String {
        format!("{}.{}.{}", self.schema, self.table, self.geometry_column)
    }

    fn to_tilejson(&self) -> TileJSON {
        create_tilejson(
            self.format_id(),
            self.minzoom,
            self.maxzoom,
            self.bounds,
            None,
        )
    }
}

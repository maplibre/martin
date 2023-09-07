use serde::{Deserialize, Serialize};
use tilejson::{Bounds, TileJSON};

use crate::config::UnrecognizedValues;
use crate::pg::config::PgInfo;
use crate::pg::utils::{patch_json, InfoMap};

pub type FuncInfoSources = InfoMap<FunctionInfo>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct FunctionInfo {
    /// Schema name
    pub schema: String,

    /// Function name
    pub function: String,

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

    /// TileJSON provided by the SQL function comment. Not serialized.
    #[serde(skip)]
    pub tilejson: Option<serde_json::Value>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl FunctionInfo {
    #[must_use]
    pub fn new(schema: String, function: String, tilejson: Option<serde_json::Value>) -> Self {
        Self {
            schema,
            function,
            tilejson,
            ..Default::default()
        }
    }

    #[must_use]
    pub fn new_extended(
        schema: String,
        function: String,
        minzoom: u8,
        maxzoom: u8,
        bounds: Bounds,
    ) -> Self {
        Self {
            schema,
            function,
            minzoom: Some(minzoom),
            maxzoom: Some(maxzoom),
            bounds: Some(bounds),
            ..Default::default()
        }
    }
}

impl PgInfo for FunctionInfo {
    fn format_id(&self) -> String {
        format!("{}.{}", self.schema, self.function)
    }

    fn to_tilejson(&self, source_id: String) -> TileJSON {
        let mut tilejson = tilejson::tilejson! {
            tiles: vec![],  // tile source is required, but not yet known
            name: source_id,
            description: self.format_id(),
        };
        tilejson.minzoom = self.minzoom;
        tilejson.maxzoom = self.maxzoom;
        tilejson.bounds = self.bounds;
        patch_json(tilejson, &self.tilejson)
    }
}

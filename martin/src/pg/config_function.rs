use serde::{Deserialize, Serialize};
use tilejson::{Bounds, TileJSON};

use crate::config::UnrecognizedValues;
use crate::pg::config::PgInfo;
use crate::pg::utils::{InfoMap, patch_json};

pub type FuncInfoSources = InfoMap<FunctionInfo>;

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct FunctionInfo {
    /// Schema name
    pub schema: String,

    /// Function name
    pub function: String,

    /// An integer specifying the minimum zoom level
    pub minzoom: Option<u8>,

    /// An integer specifying the maximum zoom level. MUST be >= minzoom
    pub maxzoom: Option<u8>,

    /// The maximum extent of available map tiles. Bounds MUST define an area
    /// covered by all zoom levels. The bounds are represented in WGS:84
    /// latitude and longitude values, in the order left, bottom, right, top.
    /// Values may be integers or floating point numbers.
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
        patch_json(tilejson, self.tilejson.as_ref())
    }
}

impl FunctionInfo {
    /// For a given function info discovered from the database, append the configuration info provided by the user
    #[must_use]
    pub fn append_cfg_info(&self, cfg_inf: &FunctionInfo) -> FunctionInfo {
        FunctionInfo {
            // TileJson does not need to be merged because it cannot be de-serialized from config
            tilejson: self.tilejson.clone(),
            ..cfg_inf.clone()
        }
    }
}

use log::error;
use serde::{Deserialize, Serialize};
use tilejson::{Bounds, TileJSON};

use crate::config::UnrecognizedValues;
use crate::pg::config::PgInfo;
use crate::pg::utils::InfoMap;

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

    /// Merge the `self.tilejson` from the function comment into the generated tilejson (param)
    fn merge_json(&self, tilejson: TileJSON) -> TileJSON {
        let Some(tj) = &self.tilejson else {
            // Nothing to merge in, keep the original
            return tilejson;
        };
        // Not the most efficient, but this is only executed once per source:
        // * Convert the TileJSON struct to a serde_json::Value
        // * Merge the self.tilejson into the value
        // * Convert the merged value back to a TileJSON struct
        // * In case of errors, return the original tilejson
        let mut tilejson2 = match serde_json::to_value(tilejson.clone()) {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to serialize tilejson, unable to merge function comment: {e}");
                return tilejson;
            }
        };
        json_patch::merge(&mut tilejson2, tj);
        match serde_json::from_value(tilejson2.clone()) {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to deserialize merged function comment tilejson: {e}");
                tilejson
            }
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
        self.merge_json(tilejson)
    }
}

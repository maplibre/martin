use std::collections::BTreeMap;

use martin_tile_utils::{Encoding, Format, TileInfo};
use serde::{Deserialize, Serialize};
use tilejson::{Bounds, TileJSON};
use tracing::warn;

use super::config::PostgresInfo;
use crate::config::file::postgres::utils::patch_json;
use crate::config::file::{CachePolicy, UnrecognizedValues};

pub type FuncInfoSources = BTreeMap<String, FunctionInfo>;

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

    /// Zoom-level bounds for tile caching.
    pub cache: Option<CachePolicy>,

    /// `TileJSON` provided by the SQL function comment. Not serialized.
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

impl PostgresInfo for FunctionInfo {
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

    /// Extract the tile format from the `content_type` field in the SQL comment JSON.
    /// Falls back to the default MVT format if `content_type` is absent or unrecognized.
    fn tile_info(&self) -> TileInfo {
        let Some(tj) = &self.tilejson else {
            return TileInfo::new(Format::Mvt, Encoding::Uncompressed);
        };
        let Some(content_type) = tj.get("content_type").and_then(|v| v.as_str()) else {
            return TileInfo::new(Format::Mvt, Encoding::Uncompressed);
        };
        if let Some(format) = Format::from_content_type(content_type) {
            TileInfo::from(format)
        } else {
            warn!(
                "Unrecognized content_type '{}' in SQL comment for {}.{}, defaulting to MVT",
                content_type, self.schema, self.function
            );
            TileInfo::new(Format::Mvt, Encoding::Uncompressed)
        }
    }
}

impl FunctionInfo {
    /// For a given function info discovered from the database, append the configuration info provided by the user
    #[must_use]
    pub fn append_cfg_info(&self, cfg_inf: &Self) -> Self {
        Self {
            // TileJson does not need to be merged because it cannot be de-serialized from config
            tilejson: self.tilejson.clone(),
            ..cfg_inf.clone()
        }
    }
}

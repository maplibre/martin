use std::collections::BTreeMap;

use martin_tile_utils::{Encoding, Format, TileInfo};
use serde::{Deserialize, Serialize};
use tilejson::{Bounds, TileJSON};
use tracing::warn;

use super::config::PostgresInfo;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::MltProcessConfig;
#[cfg(feature = "unstable-schemas")]
use crate::config::file::postgres::config_table::bounds_world_example;
use crate::config::file::postgres::utils::patch_json;
use crate::config::file::{CachePolicy, UnrecognizedValues};

pub type FuncInfoSources = BTreeMap<String, FunctionInfo>;

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct FunctionInfo {
    /// Schema name (required)
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"public"))]
    pub schema: String,

    /// Function name (required)
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"function_zxy_query"))]
    pub function: String,

    /// An integer specifying the minimum zoom level
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &0u8))]
    pub minzoom: Option<u8>,

    /// An integer specifying the maximum zoom level. MUST be >= minzoom
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &21u8))]
    pub maxzoom: Option<u8>,

    /// The maximum extent of available map tiles. Bounds MUST define an area
    /// covered by all zoom levels. The bounds are represented in WGS:84
    /// latitude and longitude values, in the order left, bottom, right, top.
    /// Values may be integers or floating point numbers.
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "Option<[f64; 4]>"))]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = bounds_world_example()))]
    pub bounds: Option<Bounds>,

    /// Zoom-level bounds for tile caching.
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<crate::config::file::CachePolicyShape>")
    )]
    pub cache: Option<CachePolicy>,

    /// `TileJSON` provided by the SQL function comment. Not serialized.
    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub tilejson: Option<serde_json::Value>,

    /// MVT->MLT encoder settings for this source.
    /// Overrides source-type and global `convert-to-mlt`.
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "convert-to-mlt"
    )]
    pub convert_to_mlt: Option<MltProcessConfig>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
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
        let (supertype, subtype) = content_type.split_once('/').unwrap_or((content_type, ""));
        if let Some(format) = Format::from_content_type(supertype, subtype) {
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

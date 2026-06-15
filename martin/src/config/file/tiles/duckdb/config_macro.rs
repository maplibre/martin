use std::collections::BTreeMap;

use martin_tile_utils::{Encoding, Format, TileInfo};
use serde::{Deserialize, Serialize};
use tilejson::{Bounds, TileJSON};

use super::DuckDbInfo;
#[cfg(feature = "unstable-schemas")]
use crate::config::file::duckdb::config_table::bounds_world_example;
use crate::config::file::duckdb::utils::patch_json;
use crate::config::file::{CachePolicy, UnrecognizedValues};
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::{MltProcessConfig, MvtProcessConfig};

pub type MacroInfoSources = BTreeMap<String, MacroInfo>;

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct MacroInfo {
    /// Schema name (required)
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"main"))]
    pub schema: String,

    /// Macro name (required)
    #[serde(rename = "macro")]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"get_custom_tile"))]
    pub macro_name: String,

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

    /// `TileJSON` provided by catalog metadata. Not serialized.
    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub tilejson: Option<serde_json::Value>,

    /// MVT->MLT encoder settings for this source.
    /// Overrides source-type and global `convert_to_mlt`.
    ///
    /// Can be either:
    /// - `null` (default) - defer to the source-type or global settings
    /// - `auto` - we choose defaults which we think work best for most users
    /// - `disabled` - no conversion
    /// - explicitly configured
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[serde(default)]
    pub convert_to_mlt: Option<MltProcessConfig>,

    /// MLT->MVT conversion settings for this source.
    /// Overrides source-type and global `convert_to_mvt`.
    ///
    /// Can be either:
    /// - `null` (default) - defer to the source-type or global settings
    /// - `auto` - we choose defaults which we think work best for most users
    /// - `disabled` - no conversion
    /// - explicitly configured
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[serde(default)]
    pub convert_to_mvt: Option<MvtProcessConfig>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl MacroInfo {
    #[must_use]
    pub fn new(schema: String, macro_name: String, tilejson: Option<serde_json::Value>) -> Self {
        Self {
            schema,
            macro_name,
            tilejson,
            ..Default::default()
        }
    }

    /// For a given macro info discovered from the database, append the configuration info provided by the user.
    #[must_use]
    pub fn append_cfg_info(&self, cfg_inf: &Self) -> Self {
        Self {
            tilejson: self.tilejson.clone(),
            ..cfg_inf.clone()
        }
    }
}

impl DuckDbInfo for MacroInfo {
    fn format_id(&self) -> String {
        format!("{}.{}", self.schema, self.macro_name)
    }

    fn to_tilejson(&self, source_id: String) -> TileJSON {
        let mut tilejson = tilejson::tilejson! {
            tiles: vec![],
            name: source_id,
            description: self.format_id(),
        };
        tilejson.minzoom = self.minzoom;
        tilejson.maxzoom = self.maxzoom;
        tilejson.bounds = self.bounds;
        patch_json(tilejson, self.tilejson.as_ref())
    }

    fn tile_info(&self) -> TileInfo {
        TileInfo::new(Format::Mvt, Encoding::Uncompressed)
    }
}

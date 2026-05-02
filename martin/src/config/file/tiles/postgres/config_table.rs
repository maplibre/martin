use std::collections::{BTreeMap, HashMap};

use martin_tile_utils::{Encoding, Format, TileInfo};
use serde::{Deserialize, Serialize};
use tilejson::{Bounds, TileJSON, VectorLayer};
use tracing::{info, warn};

use super::PostgresInfo;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::MltProcessConfig;
use crate::config::file::postgres::utils::{normalize_key, patch_json};
use crate::config::file::{CachePolicy, UnrecognizedValues};

pub type TableInfoSources = BTreeMap<String, TableInfo>;

/// Example bounds covering the whole world, shared between table and function
/// configs so the rendered docs example matches what the curated `config.yaml`
/// used to ship.
#[cfg(feature = "unstable-schemas")]
pub(crate) fn bounds_world_example() -> [f64; 4] {
    [-180.0, -90.0, 180.0, 90.0]
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct TableInfo {
    /// ID of the MVT layer (optional, defaults to table name)
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"table_source"))]
    pub layer_id: Option<String>,

    /// Table schema (required)
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"public"))]
    pub schema: String,

    /// Table name (required)
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"table_source"))]
    pub table: String,

    /// Geometry SRID (required)
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &4326i32))]
    pub srid: i32,

    /// Geometry column name (required)
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"geom"))]
    pub geometry_column: String,

    /// Geometry column has a spatial index
    #[serde(skip)]
    pub geometry_index: Option<bool>,

    /// Flag indicating the `PostgreSQL relkind`:
    /// - `"t"`: Table
    /// - `"v"`: View
    /// - `"m"`: Materialized View
    #[serde(skip)]
    pub relkind: Option<char>,

    /// Feature id column name
    pub id_column: Option<String>,

    /// An integer specifying the minimum zoom level
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &0u8))]
    pub minzoom: Option<u8>,

    /// An integer specifying the maximum zoom level. MUST be >= minzoom
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &30u8))]
    pub maxzoom: Option<u8>,

    /// The maximum extent of available map tiles. Bounds MUST define an area
    /// covered by all zoom levels. The bounds are represented in WGS:84 latitude
    /// and longitude values, in the order left, bottom, right, top. Values may
    /// be integers or floating point numbers.
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "Option<[f64; 4]>"))]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(example = bounds_world_example())
    )]
    pub bounds: Option<Bounds>,

    /// Tile extent in tile coordinate space
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &4096u32))]
    pub extent: Option<u32>,

    /// Buffer distance in tile coordinate space to optionally clip geometries
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &64u32))]
    pub buffer: Option<u32>,

    /// Boolean to control if geometries should be clipped or encoded as is
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &true))]
    pub clip_geom: Option<bool>,

    /// Geometry type
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"GEOMETRY"))]
    pub geometry_type: Option<String>,

    /// Zoom-level bounds for tile caching (overrides top-level cache).
    /// default: null (inherit from top-level default)
    /// Use `cache: disable` to disable caching for this source.
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<crate::config::file::CachePolicyShape>")
    )]
    pub cache: Option<CachePolicy>,

    /// List of columns, that should be encoded as tile properties (required)
    ///
    /// Keys and values are the names and descriptions of attributes available in this layer.
    /// Each value (description) must be a string that describes the underlying data.
    /// If no fields (=just the geometry) should be encoded, an empty object is allowed.
    pub properties: Option<BTreeMap<String, String>>,

    /// Mapping of properties to the actual table columns
    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub prop_mapping: HashMap<String, String>,

    /// MVT→MLT encoder settings for this source.
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

    /// `TileJSON` provider by the SQL comment. Shouldn't be serialized
    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub tilejson: Option<serde_json::Value>,
}

impl PostgresInfo for TableInfo {
    fn format_id(&self) -> String {
        format!("{}.{}.{}", self.schema, self.table, self.geometry_column)
    }

    /// Result `TileJson` will be patched by the `TileJson` from SQL comment if provided.
    /// The `source_id` will be replaced by `self.layer_id` in the vector layer info if set.
    fn to_tilejson(&self, source_id: String) -> TileJSON {
        let mut tilejson = tilejson::tilejson! {
            tiles: vec![],  // tile source is required, but not yet known
            name: source_id.clone(),
            description: self.format_id(),
        };
        tilejson.minzoom = self.minzoom;
        tilejson.maxzoom = self.maxzoom;
        tilejson.bounds = self.bounds;

        let id = if let Some(id) = &self.layer_id {
            id.clone()
        } else {
            source_id
        };

        let layer = VectorLayer {
            id,
            fields: self.properties.clone().unwrap_or_default(),
            description: None,
            maxzoom: None,
            minzoom: None,
            other: BTreeMap::default(),
        };
        tilejson.vector_layers = Some(vec![layer]);
        patch_json(tilejson, self.tilejson.as_ref())
    }

    fn tile_info(&self) -> TileInfo {
        TileInfo::new(Format::Mvt, Encoding::Uncompressed)
    }
}

impl TableInfo {
    /// For a given table info discovered from the database, append the configuration info provided by the user
    #[must_use]
    pub fn append_cfg_info(
        &self,
        cfg_inf: &Self,
        new_id: &String,
        default_srid: Option<i32>,
    ) -> Option<Self> {
        // Assume cfg_inf and self have the same schema/table/geometry_column
        let mut inf = Self {
            // These values must match the database exactly
            schema: self.schema.clone(),
            table: self.table.clone(),
            geometry_column: self.geometry_column.clone(),
            // These values are not serialized, so copy auto-detected values from the database
            geometry_index: self.geometry_index,
            relkind: self.relkind,
            tilejson: self.tilejson.clone(),
            // Srid requires some logic
            srid: self.calc_srid(new_id, cfg_inf.srid, default_srid)?,
            prop_mapping: HashMap::new(),
            ..cfg_inf.clone()
        };

        match (&self.geometry_type, &cfg_inf.geometry_type) {
            (Some(src), Some(cfg)) if src != cfg => {
                warn!(
                    r"Table {} has geometry type={src}, but source {new_id} has {cfg}",
                    self.format_id()
                );
            }
            _ => {}
        }

        let empty = BTreeMap::new();
        let props = self.properties.as_ref().unwrap_or(&empty);

        if let Some(id_column) = &cfg_inf.id_column {
            let prop = normalize_key(props, id_column.as_str(), "id_column", new_id)?;
            inf.prop_mapping.insert(id_column.clone(), prop);
        }

        if let Some(p) = &cfg_inf.properties {
            for key in p.keys() {
                let prop = normalize_key(props, key.as_str(), "property", new_id)?;
                inf.prop_mapping.insert(key.clone(), prop);
            }
        }

        Some(inf)
    }

    /// Determine the SRID value to use for a table, or None if unknown, assuming self is a table info from the database
    ///
    /// Tries to use `default_srid` if a spatial table has SRID 0.
    #[must_use]
    pub fn calc_srid(&self, new_id: &str, cfg_srid: i32, default_srid: Option<i32>) -> Option<i32> {
        match (self.srid, cfg_srid, default_srid) {
            (0, 0, Some(default_srid)) => {
                info!(
                    "Table {} has SRID=0, using provided default SRID={default_srid}",
                    self.format_id()
                );
                Some(default_srid)
            }
            (0, 0, None) => {
                let info = "To use this table source, set default or specify this table SRID in the config file, or set the default SRID with  --default-srid=...";
                warn!("Table {} has SRID=0, skipping. {info}", self.format_id());
                None
            }
            (0, cfg, _) => Some(cfg), // Use the configured SRID
            (src, 0, _) => Some(src), // Use the source SRID
            (src, cfg, _) if src != cfg => {
                warn!(
                    "Table {} has SRID={src}, but source {new_id} has SRID={cfg}",
                    self.format_id()
                );
                None
            }
            (_, cfg, _) => Some(cfg),
        }
    }
}

use std::collections::{BTreeMap, HashMap};

use log::{info, warn};
use serde::{Deserialize, Serialize};
use tilejson::{Bounds, TileJSON, VectorLayer};

use crate::config::UnrecognizedValues;
use crate::pg::config::PgInfo;
use crate::pg::utils::{normalize_key, patch_json, InfoMap};

pub type TableInfoSources = InfoMap<TableInfo>;

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct TableInfo {
    /// ID of the layer as specified in a tile (ST_AsMVT param)
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
    #[serde(skip)]
    pub geometry_index: Option<bool>,

    /// Flag indicating if table is actually a view (PostgreSQL relkind = 'v')
    #[serde(skip)]
    pub is_view: Option<bool>,

    /// Feature id column name
    pub id_column: Option<String>,

    /// An integer specifying the minimum zoom level
    pub minzoom: Option<u8>,

    /// An integer specifying the maximum zoom level. MUST be >= minzoom
    pub maxzoom: Option<u8>,

    /// The maximum extent of available map tiles. Bounds MUST define an area
    /// covered by all zoom levels. The bounds are represented in WGS:84
    /// latitude and longitude values, in the order left, bottom, right, top.
    /// Values may be integers or floating point numbers.
    pub bounds: Option<Bounds>,

    /// Tile extent in tile coordinate space
    pub extent: Option<u32>,

    /// Buffer distance in tile coordinate space to optionally clip geometries
    pub buffer: Option<u32>,

    /// Boolean to control if geometries should be clipped or encoded as is
    pub clip_geom: Option<bool>,

    /// Geometry type
    pub geometry_type: Option<String>,

    /// List of columns, that should be encoded as tile properties
    pub properties: Option<BTreeMap<String, String>>,

    /// Mapping of properties to the actual table columns
    #[serde(skip)]
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
}

impl TableInfo {
    /// For a given table info discovered from the database, append the configuration info provided by the user
    #[must_use]
    pub fn append_cfg_info(
        &self,
        cfg_inf: &TableInfo,
        new_id: &String,
        default_srid: Option<i32>,
    ) -> Option<Self> {
        // Assume cfg_inf and self have the same schema/table/geometry_column
        let mut inf = TableInfo {
            // These values must match the database exactly
            schema: self.schema.clone(),
            table: self.table.clone(),
            geometry_column: self.geometry_column.clone(),
            // These values are not serialized, so copy auto-detected values from the database
            geometry_index: self.geometry_index,
            is_view: self.is_view,
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

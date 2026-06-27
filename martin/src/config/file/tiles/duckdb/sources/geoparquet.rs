use std::num::{NonZeroU32};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::file::tiles::duckdb::sources::DuckDbSourceSettings;
use crate::config::file::{ConfigurationLivecycleHooks, UnrecognizedKeys, UnrecognizedValues};

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct GeoParquetEntry {
    /// Local path or remote URL of the GeoParquet source.
    pub geoparquet: PathBuf,
    /// Optional output source/layer identifier override.
    pub layer_id: Option<String>,
    /// Optional feature id column to use as MVT feature id.
    pub id_column: Option<String>,
    /// Optional geometry column name. Auto-detected when omitted.
    pub geometry_column: Option<String>,
    /// Optional source SRID. Auto-detected when omitted.
    pub srid: Option<i32>,
    /// Optional minimum zoom for source metadata.
    pub minzoom: Option<u8>,
    /// Optional maximum zoom for source metadata.
    pub maxzoom: Option<u8>,
    /// Optional tile extent (MVT coordinate space).
    pub extent: Option<NonZeroU32>,
    /// Optional geometry buffer in tile coordinate space.
    pub buffer: Option<u32>,
    /// Optional geometry clipping toggle.
    pub clip_geom: Option<bool>,
    #[serde(flatten)]
    pub settings: DuckDbSourceSettings,
    /// Unknown keys preserved for diagnostics.
    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl GeoParquetEntry {
    pub fn finalize(&mut self) {
        if self.id_column.as_deref() == Some("") {
            self.id_column = None;
        }
        if self.layer_id.as_deref() == Some("") {
            self.layer_id = None;
        }
        if self.geometry_column.as_deref() == Some("") {
            self.geometry_column = None;
        }
    }
}

impl ConfigurationLivecycleHooks for GeoParquetEntry {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}

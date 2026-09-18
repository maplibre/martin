use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

use crate::config::file::CollectUnrecognizedKeys;

/// How one relation is encoded into an MVT layer, shared by every `DuckDB` source kind.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct MvtLayerOptions {
    /// Optional feature id column to use as MVT feature id.
    pub id_column: Option<String>,
    /// Optional geometry column name. Auto-detected when omitted.
    pub geometry_column: Option<String>,
    /// Optional source SRID. Auto-detected when omitted.
    /// Non-positive values are treated as unset and fall back to auto-detection.
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
}

impl MvtLayerOptions {
    pub(crate) fn finalize(&mut self) {
        if self.id_column.as_deref() == Some("") {
            self.id_column = None;
        }
        if self.geometry_column.as_deref() == Some("") {
            self.geometry_column = None;
        }
        if let Some(srid) = self.srid
            && srid <= 0
        {
            self.srid = None;
        }
    }
}

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tilejson::Bounds;

use crate::config::file::tiles::duckdb::sources::auto_publish::{MacroDiscovery, TableDiscovery};
use crate::config::file::tiles::duckdb::sources::{
    DuckDbCfgPublish, DuckDbSourceSettings, MvtLayerOptions,
};
use crate::config::file::{
    CollectUnrecognizedKeys, ConfigFileError, ConfigFileResult, UnrecognizedValues,
};
use crate::config::primitives::OptBoolObj;

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct DuckDbDatabaseEntry {
    /// Path of the `.duckdb` database file (wire / saved-config form).
    pub database: PathBuf,
    /// Canonical path filled by [`DuckDbDatabaseEntry::finalize`]. Not serialized.
    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub(crate) path: Option<PathBuf>,
    #[serde(flatten)]
    pub settings: DuckDbSourceSettings,
    /// Automatic discovery of the database's tables. \[default: null\]
    ///
    /// Options:
    /// - `true`: run automatic discovery (`true` may be omitted if further configuration is provided)
    /// - `false`: disable automatic discovery
    /// - null: run automatic discovery if `tables` and `macros` are null
    #[serde(default, skip_serializing_if = "OptBoolObj::is_none")]
    pub auto_publish: OptBoolObj<DuckDbCfgPublish>,
    /// Tables of this database to publish, keyed by source id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tables: Option<BTreeMap<String, DuckDbTableEntry>>,
    /// Table macros of this database to publish, keyed by source id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macros: Option<BTreeMap<String, DuckDbMacroEntry>>,
    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl DuckDbDatabaseEntry {
    pub fn finalize(&mut self) -> ConfigFileResult<()> {
        let canonical = self
            .database
            .canonicalize()
            .map_err(|error| ConfigFileError::IoError(error, self.database.clone()))?;
        if canonical.is_dir() {
            return Err(ConfigFileError::InvalidFilePath(canonical));
        }
        self.path = Some(canonical);
        for table in self.tables.iter_mut().flat_map(BTreeMap::values_mut) {
            table.finalize();
        }
        for r#macro in self.macros.iter_mut().flat_map(BTreeMap::values_mut) {
            r#macro.finalize();
        }
        if self.tables.is_none() && self.macros.is_none() && self.auto_publish.is_none() {
            self.auto_publish = OptBoolObj::Bool(true);
        }
        Ok(())
    }

    /// Table discovery settings, or `None` when tables are not auto-published.
    pub(crate) fn table_discovery(&self) -> Option<TableDiscovery> {
        DuckDbCfgPublish::table_discovery(&self.auto_publish)
    }

    /// Macro discovery settings, or `None` when macros are not auto-published.
    pub(crate) fn macro_discovery(&self) -> Option<MacroDiscovery> {
        DuckDbCfgPublish::macro_discovery(&self.auto_publish)
    }

    /// Source id stem, from the database file name.
    #[must_use]
    pub fn stem(&self) -> &str {
        self.database
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("duckdb")
    }
}

/// One table of a `DuckDB` database file, published as an MVT source.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct DuckDbTableEntry {
    /// Schema the table lives in. Defaults to `main`.
    pub schema: Option<String>,
    /// Table name.
    pub table: String,
    #[serde(flatten)]
    pub layer: MvtLayerOptions,
    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl DuckDbTableEntry {
    pub(crate) fn finalize(&mut self) {
        if self.schema.as_deref() == Some("") {
            self.schema = None;
        }
        self.layer.finalize();
    }

    #[must_use]
    pub fn schema(&self) -> &str {
        self.schema.as_deref().unwrap_or("main")
    }
}

/// One table macro of a `DuckDB` database file, published as a tile source.
///
/// The macro takes `(z, x, y)` and returns a single row whose first column is the tile.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct DuckDbMacroEntry {
    /// Schema the macro lives in. Defaults to `main`.
    pub schema: Option<String>,
    /// Macro name.
    pub r#macro: String,
    /// An integer specifying the minimum zoom level
    pub minzoom: Option<u8>,
    /// An integer specifying the maximum zoom level. MUST be >= minzoom
    pub maxzoom: Option<u8>,
    /// The maximum extent of available map tiles. Bounds MUST define an area
    /// covered by all zoom levels. The bounds are represented in WGS:84
    /// latitude and longitude values, in the order left, bottom, right, top.
    /// Values may be integers or floating point numbers.
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "Option<[f64; 4]>"))]
    pub bounds: Option<Bounds>,
    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl DuckDbMacroEntry {
    pub(crate) fn finalize(&mut self) {
        if self.schema.as_deref() == Some("") {
            self.schema = None;
        }
    }

    #[must_use]
    pub fn schema(&self) -> &str {
        self.schema.as_deref().unwrap_or("main")
    }
}

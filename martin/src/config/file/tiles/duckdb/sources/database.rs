use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::file::UnrecognizedValues;
use crate::config::file::tiles::duckdb::sources::DuckDbSourceSettings;

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct DuckDbDatabaseEntry {
    pub database: PathBuf,
    #[serde(flatten)]
    pub settings: DuckDbSourceSettings,
    /// Auto-publish config block for database-backed sources.
    /// Kept as opaque in PR1; structured parsing lands with database sources in later PRs.
    pub auto_publish: Option<serde_json::Value>,
    /// Explicit table source map for this database entry.
    /// Kept as opaque in PR1; structured parsing lands in later PRs.
    pub tables: Option<serde_json::Value>,
    /// Explicit macro source map for this database entry.
    /// Kept as opaque in PR1; structured parsing lands in later PRs.
    pub macros: Option<serde_json::Value>,
    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl DuckDbDatabaseEntry {
    pub fn finalize(&mut self) {}
}

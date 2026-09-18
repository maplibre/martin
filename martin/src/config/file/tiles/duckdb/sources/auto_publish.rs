use std::collections::HashSet;
use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

use crate::config::file::{CollectUnrecognizedKeys, UnrecognizedValues};
use crate::config::primitives::OptBoolObj::{self, Bool, NoValue, Object};
use crate::config::primitives::OptOneMany::{self, NoVals};

/// Automatic discovery of the tables and table macros of a `DuckDB` database file.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct DuckDbCfgPublish {
    /// Optionally limit to just these schemas
    #[serde(alias = "from_schema")]
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub from_schemas: OptOneMany<String>,
    /// Here we enable both tables and macros auto discovery.
    /// You can also enable just one of them by not mentioning the other, or
    /// setting it to false. Setting one to true disables the other one as well.
    /// E.g. `tables: false` enables just the macros auto-discovery.
    #[serde(default, skip_serializing_if = "OptBoolObj::is_none")]
    pub tables: OptBoolObj<DuckDbCfgPublishTables>,
    #[serde(default, skip_serializing_if = "OptBoolObj::is_none")]
    pub macros: OptBoolObj<DuckDbCfgPublishMacros>,
    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct DuckDbCfgPublishMacros {
    /// Optionally limit to just these schemas
    #[serde(alias = "from_schema")]
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub from_schemas: OptOneMany<String>,
    /// Optionally set how source ID should be generated based on the macro's
    /// name and schema
    #[serde(alias = "id_format")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(example = &"{schema}.{macro}")
    )]
    pub source_id_format: Option<String>,
    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct DuckDbCfgPublishTables {
    /// Add more schemas to the ones listed above
    #[serde(alias = "from_schema")]
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub from_schemas: OptOneMany<String>,
    /// Optionally set how source ID should be generated based on the table's name,
    /// schema, and geometry column
    #[serde(alias = "id_format")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(example = &"{schema}.{table}.{column}")
    )]
    pub source_id_format: Option<String>,
    /// A table column to use as the feature ID
    /// If a table has no column with this name, `id_column` will not be set for
    /// that table.
    /// If a list of strings is given, the first found column will be treated as a
    /// feature ID.
    #[serde(alias = "id_column")]
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub id_columns: OptOneMany<String>,
    /// Controls if geometries should be clipped or encoded as is \[default: true\]
    pub clip_geom: Option<bool>,
    /// Buffer distance in tile coordinate space to optionally clip geometries,
    /// optional, default to 64
    pub buffer: Option<u32>,
    /// Tile extent in tile coordinate space, optional, default to 4096
    pub extent: Option<NonZeroU32>,
    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

/// The effective table discovery settings of one database entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TableDiscovery {
    /// Schemas to search, or every schema when `None`.
    pub schemas: Option<HashSet<String>>,
    pub source_id_format: String,
    pub id_columns: Option<Vec<String>>,
    pub clip_geom: Option<bool>,
    pub buffer: Option<u32>,
    pub extent: Option<NonZeroU32>,
}

/// The effective macro discovery settings of one database entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MacroDiscovery {
    /// Schemas to search, or every schema when `None`.
    pub schemas: Option<HashSet<String>>,
    pub source_id_format: String,
}

fn merge_schemas(
    outer: &OptOneMany<String>,
    inner: &OptOneMany<String>,
) -> Option<HashSet<String>> {
    match (outer, inner) {
        (NoVals, NoVals) => None,
        (outer, inner) => Some(outer.iter().chain(inner.iter()).cloned().collect()),
    }
}

impl DuckDbCfgPublish {
    /// Table discovery settings, or `None` when tables are not auto-published.
    pub(crate) fn table_discovery(auto_publish: &OptBoolObj<Self>) -> Option<TableDiscovery> {
        match auto_publish {
            NoValue | Bool(false) => None,
            Bool(true) => Some(TableDiscovery::default()),
            Object(publish) => match &publish.tables {
                Bool(false) => None,
                NoValue if !matches!(publish.macros, NoValue | Bool(false)) => None,
                NoValue | Bool(true) => Some(TableDiscovery {
                    schemas: merge_schemas(&publish.from_schemas, &NoVals),
                    ..TableDiscovery::default()
                }),
                Object(tables) => Some(TableDiscovery {
                    schemas: merge_schemas(&publish.from_schemas, &tables.from_schemas),
                    source_id_format: tables
                        .source_id_format
                        .clone()
                        .unwrap_or_else(|| DEFAULT_TABLE_ID_FORMAT.to_owned()),
                    id_columns: tables.id_columns.opt_iter().map(|v| v.cloned().collect()),
                    clip_geom: tables.clip_geom,
                    buffer: tables.buffer,
                    extent: tables.extent,
                }),
            },
        }
    }

    /// Macro discovery settings, or `None` when macros are not auto-published.
    pub(crate) fn macro_discovery(auto_publish: &OptBoolObj<Self>) -> Option<MacroDiscovery> {
        match auto_publish {
            NoValue | Bool(false) => None,
            Bool(true) => Some(MacroDiscovery::default()),
            Object(publish) => match &publish.macros {
                Bool(false) => None,
                NoValue if !matches!(publish.tables, NoValue | Bool(false)) => None,
                NoValue | Bool(true) => Some(MacroDiscovery {
                    schemas: merge_schemas(&publish.from_schemas, &NoVals),
                    ..MacroDiscovery::default()
                }),
                Object(macros) => Some(MacroDiscovery {
                    schemas: merge_schemas(&publish.from_schemas, &macros.from_schemas),
                    source_id_format: macros
                        .source_id_format
                        .clone()
                        .unwrap_or_else(|| DEFAULT_MACRO_ID_FORMAT.to_owned()),
                }),
            },
        }
    }
}

const DEFAULT_TABLE_ID_FORMAT: &str = "{table}";
const DEFAULT_MACRO_ID_FORMAT: &str = "{macro}";

impl Default for MacroDiscovery {
    fn default() -> Self {
        Self {
            schemas: None,
            source_id_format: DEFAULT_MACRO_ID_FORMAT.to_owned(),
        }
    }
}

impl Default for TableDiscovery {
    fn default() -> Self {
        Self {
            schemas: None,
            source_id_format: DEFAULT_TABLE_ID_FORMAT.to_owned(),
            id_columns: None,
            clip_geom: None,
            buffer: None,
            extent: None,
        }
    }
}

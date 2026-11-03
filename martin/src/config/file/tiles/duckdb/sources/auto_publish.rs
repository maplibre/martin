use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

use crate::config::file::{CollectUnrecognizedKeys, UnrecognizedValues};
use crate::config::primitives::{MacroIdFormat, OptBoolObj, TableIdFormat, one_or_many};

/// Automatic discovery of the tables and table macros of a `DuckDB` database file.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct DuckDbCfgPublish {
    /// Optionally limit to just these schemas
    #[serde(alias = "from_schema")]
    #[serde(default, with = "one_or_many", skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "one_or_many::OneOrMany<String>"))]
    pub from_schemas: Vec<String>,
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
    #[serde(default, with = "one_or_many", skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "one_or_many::OneOrMany<String>"))]
    pub from_schemas: Vec<String>,
    /// Optionally set how source ID should be generated based on the macro's
    /// name and schema
    #[serde(alias = "id_format")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<String>", example = &"{schema}.{macro}")
    )]
    pub source_id_format: Option<MacroIdFormat>,
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
    #[serde(default, with = "one_or_many", skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "one_or_many::OneOrMany<String>"))]
    pub from_schemas: Vec<String>,
    /// Optionally set how source ID should be generated based on the table's name,
    /// schema, and geometry column
    #[serde(alias = "id_format")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<String>", example = &"{schema}.{table}.{column}")
    )]
    pub source_id_format: Option<TableIdFormat>,
    /// A table column to use as the feature ID
    /// If a table has no column with this name, `id_column` will not be set for
    /// that table.
    /// If a list of strings is given, the first found column will be treated as a
    /// feature ID.
    #[serde(alias = "id_column")]
    #[serde(default, with = "one_or_many", skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "one_or_many::OneOrMany<String>"))]
    pub id_columns: Vec<String>,
    /// Controls if geometries should be clipped or encoded as is \[default: true\]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &true))]
    pub clip_geom: Option<bool>,
    /// Buffer distance in tile coordinate space to optionally clip geometries,
    /// optional, default to 64
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &64u32))]
    pub buffer: Option<u32>,
    /// Tile extent in tile coordinate space, optional, default to 4096
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &4096u32))]
    pub extent: Option<NonZeroU32>,
    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

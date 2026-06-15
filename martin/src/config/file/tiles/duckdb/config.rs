use std::path::PathBuf;

use martin_tile_utils::TileInfo;
use serde::{Deserialize, Serialize};
use tilejson::TileJSON;

use super::{DuckDbGeoParquetSourceConfig, MacroInfoSources, TableInfoSources};
use crate::config::args::BoundsCalcType;
use crate::config::file::{
    CachePolicy, ConfigFileError, ConfigFileResult, ConfigurationLivecycleHooks, ResolutionResult,
    UnrecognizedKeys, UnrecognizedValues, copy_unrecognized_keys_from_config,
};
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::{MltProcessConfig, MvtProcessConfig};
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::primitives::AutoOption;
use crate::config::primitives::{IdResolver, OptBoolObj, OptOneMany};

/// Default DuckDB connection pool size for tile serving.
pub const DEFAULT_POOL_SIZE: usize = 4;

pub trait DuckDbInfo {
    fn format_id(&self) -> String;
    fn to_tilejson(&self, source_id: String) -> TileJSON;
    /// Return the tile format and encoding for this source.
    fn tile_info(&self) -> TileInfo;
}

/// Top-level DuckDB configuration block.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct DuckDbConfig {
    /// Maximum DuckDB connection pool size \[default: 4\]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &4usize))]
    pub pool_size: Option<usize>,
    /// Specify how bounds should be computed for spatial sources \[default: quick\]
    ///
    /// Options:
    /// - `calc` compute geometry bounds on startup.
    /// - `quick` same as 'calc', but the calculation will be aborted after 5 seconds.
    /// - `skip` does not compute geometry bounds on startup.
    pub auto_bounds: Option<BoundsCalcType>,
    /// If a spatial table has SRID 0, this SRID is used as a fallback.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &4326i32))]
    pub default_srid: Option<i32>,
    /// DuckDB threads per query. Omitted values defer to DuckDB defaults (nCPU).
    pub threads_per_query: Option<usize>,
    /// DuckDB memory limit in megabytes. Omitted values defer to DuckDB defaults (80% RAM).
    pub memory_limit_mb: Option<usize>,
    /// DuckDB source entries (database files and GeoParquet datasets).
    #[serde(default)]
    pub sources: Vec<DuckDbSourceConfig>,
    /// MVT->MLT encoder settings for all sources from this connection.
    /// Overrides global; overridden by per-source `convert_to_mlt`.
    ///
    /// Can be either:
    /// - `null` (default) - defer to the global setting
    /// - `auto` - we choose defaults which we think work best for most users
    /// - `disabled` - no conversion
    /// - explicitely configured
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[serde(default)]
    pub convert_to_mlt: Option<MltProcessConfig>,

    /// MLT->MVT conversion settings for all sources from this connection.
    /// Overrides global; overridden by per-source `convert_to_mvt`.
    ///
    /// Can be either:
    /// - `null` (default) - defer to the global setting
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

impl Default for DuckDbConfig {
    fn default() -> Self {
        Self {
            pool_size: Some(DEFAULT_POOL_SIZE),
            auto_bounds: None,
            default_srid: None,
            threads_per_query: None,
            memory_limit_mb: None,
            sources: Vec::new(),
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            convert_to_mlt: None,
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            convert_to_mvt: None,
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

/// One configured DuckDB source entry under `duckdb.sources`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DuckDbSourceConfig {
    Database(DuckDbDatabaseSourceConfig),
    GeoParquet(DuckDbGeoParquetSourceConfig),
}

/// Database-file source entry under `duckdb.sources`.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct DuckDbDatabaseSourceConfig {
    /// Path to the DuckDB database file.
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(example = &"/data/tiles.duckdb")
    )]
    pub database: PathBuf,
    /// Override wrapper-level pool size for this database \[default: inherit\]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &4usize))]
    pub pool_size: Option<usize>,
    /// Override wrapper-level bounds calculation for this database \[default: inherit\]
    pub auto_bounds: Option<BoundsCalcType>,
    /// Override wrapper-level query thread count for this database \[default: inherit\]
    pub threads_per_query: Option<usize>,
    /// Override wrapper-level memory limit for this database \[default: inherit\]
    pub memory_limit_mb: Option<usize>,
    /// Enable automatic discovery of tables and macros. \[default: null\]
    ///
    /// Options:
    /// - `true`: run automatic discovery (`true` may be omitted if further configuration is provided)
    /// - `false`: disable automatic discovery
    /// - null: run automatic discovery if `tables` and `macros` are null
    #[serde(default, skip_serializing_if = "OptBoolObj::is_none")]
    pub auto_publish: OptBoolObj<DuckDbCfgPublish>,
    /// Associative array of table sources keyed by source ID
    pub tables: Option<TableInfoSources>,
    /// Associative array of macro sources keyed by source ID
    pub macros: Option<MacroInfoSources>,

    /// MVT->MLT encoder settings for all sources from this database file.
    /// Overrides wrapper-level and global settings; overridden by per-source `convert_to_mlt`.
    ///
    /// Can be either:
    /// - `null` (default) - defer to the wrapper or global setting
    /// - `auto` - we choose defaults which we think work best for most users
    /// - `disabled` - no conversion
    /// - explicitly configured
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[serde(default)]
    pub convert_to_mlt: Option<MltProcessConfig>,

    /// MLT->MVT conversion settings for all sources from this database file.
    /// Overrides wrapper-level and global settings; overridden by per-source `convert_to_mvt`.
    ///
    /// Can be either:
    /// - `null` (default) - defer to the wrapper or global setting
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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
        schemars(example = &"{table}")
    )]
    pub source_id_format: Option<String>,
    /// A table column to use as the feature ID.
    /// If a table has no column with this name, `id_column` will not be set for
    /// that table.
    /// If a list of strings is given, the first found column will be treated as a
    /// feature ID.
    #[serde(alias = "id_column")]
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub id_columns: OptOneMany<String>,
    /// Controls if geometries should be clipped or encoded as is \[default: true\]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &true))]
    pub clip_geom: Option<bool>,
    /// Buffer distance in tile coordinate space to optionally clip geometries,
    /// optional, default to 64
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &64u32))]
    pub buffer: Option<u32>,
    /// Tile extent in tile coordinate space, optional, default to 4096
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &4096u32))]
    pub extent: Option<std::num::NonZeroU32>,
    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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
        schemars(example = &"{macro}")
    )]
    pub source_id_format: Option<String>,
    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigurationLivecycleHooks for DuckDbCfgPublish {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        let mut keys = self
            .unrecognized
            .keys()
            .cloned()
            .collect::<UnrecognizedKeys>();
        match &self.tables {
            OptBoolObj::NoValue | OptBoolObj::Bool(_) => {}
            OptBoolObj::Object(o) => keys.extend(
                o.get_unrecognized_keys()
                    .iter()
                    .map(|k| format!("tables.{k}")),
            ),
        }
        match &self.macros {
            OptBoolObj::NoValue | OptBoolObj::Bool(_) => {}
            OptBoolObj::Object(o) => keys.extend(
                o.get_unrecognized_keys()
                    .iter()
                    .map(|k| format!("macros.{k}")),
            ),
        }
        keys
    }
}

impl ConfigurationLivecycleHooks for DuckDbCfgPublishTables {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}

impl ConfigurationLivecycleHooks for DuckDbCfgPublishMacros {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}

impl DuckDbSourceConfig {
    #[must_use]
    pub fn pool_size(&self) -> Option<usize> {
        match self {
            Self::Database(cfg) => cfg.pool_size,
            Self::GeoParquet(cfg) => cfg.pool_size,
        }
    }

    #[must_use]
    pub fn auto_bounds(&self) -> Option<BoundsCalcType> {
        match self {
            Self::Database(cfg) => cfg.auto_bounds,
            Self::GeoParquet(cfg) => cfg.auto_bounds,
        }
    }

    #[must_use]
    pub fn threads_per_query(&self) -> Option<usize> {
        match self {
            Self::Database(cfg) => cfg.threads_per_query,
            Self::GeoParquet(cfg) => cfg.threads_per_query,
        }
    }

    #[must_use]
    pub fn memory_limit_mb(&self) -> Option<usize> {
        match self {
            Self::Database(cfg) => cfg.memory_limit_mb,
            Self::GeoParquet(cfg) => cfg.memory_limit_mb,
        }
    }
}

impl DuckDbConfig {
    #[must_use]
    pub fn effective_pool_size(&self, source: &DuckDbSourceConfig) -> usize {
        source
            .pool_size()
            .or(self.pool_size)
            .unwrap_or(DEFAULT_POOL_SIZE)
    }

    #[must_use]
    pub fn effective_auto_bounds(&self, source: &DuckDbSourceConfig) -> BoundsCalcType {
        source
            .auto_bounds()
            .or(self.auto_bounds)
            .unwrap_or_default()
    }

    #[must_use]
    pub fn effective_threads_per_query(&self, source: &DuckDbSourceConfig) -> Option<usize> {
        source.threads_per_query().or(self.threads_per_query)
    }

    #[must_use]
    pub fn effective_memory_limit_mb(&self, source: &DuckDbSourceConfig) -> Option<usize> {
        source.memory_limit_mb().or(self.memory_limit_mb)
    }

    pub fn validate(&self) -> Result<(), String> {
        for source in &self.sources {
            source.validate()?;
        }
        Ok(())
    }

    pub async fn resolve(
        &mut self,
        _id_resolver: IdResolver,
        _default_cache: CachePolicy,
    ) -> ResolutionResult {
        //TODO: Discovery and instantiation to be implemented
        Ok((Vec::new(), Vec::new()))
    }
}

impl ConfigurationLivecycleHooks for DuckDbConfig {
    fn finalize(&mut self) -> ConfigFileResult<()> {
        if self.pool_size.is_some_and(|size| size < 1) {
            return Err(ConfigFileError::DuckDbPoolSizeInvalid);
        }
        if self.sources.is_empty() {
            return Err(ConfigFileError::DuckDbSourcesEmpty);
        }

        for source in &mut self.sources {
            source.finalize()?;
        }

        Ok(())
    }

    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        let mut keys = self
            .unrecognized
            .keys()
            .cloned()
            .collect::<UnrecognizedKeys>();

        for (idx, source) in self.sources.iter().enumerate() {
            let prefix = format!("sources[{idx}].");
            keys.extend(source.get_unrecognized_keys_with_prefix(&prefix));
        }

        #[cfg(all(feature = "mlt", feature = "_tiles"))]
        {
            if let Some(AutoOption::Explicit(cfg)) = self.convert_to_mlt.as_ref() {
                keys.extend(
                    cfg.unrecognized_keys()
                        .map(|k| format!("convert_to_mlt.{k}")),
                );
            }
            if let Some(AutoOption::Explicit(cfg)) = self.convert_to_mvt.as_ref() {
                keys.extend(
                    cfg.unrecognized_keys()
                        .map(|k| format!("convert_to_mvt.{k}")),
                );
            }
        }

        keys
    }
}

impl DuckDbSourceConfig {
    fn finalize(&mut self) -> ConfigFileResult<()> {
        if self.pool_size().is_some_and(|size| size < 1) {
            return Err(ConfigFileError::DuckDbPoolSizeInvalid);
        }

        match self {
            Self::Database(cfg) => {
                if cfg.tables.is_none() && cfg.macros.is_none() && cfg.auto_publish.is_none() {
                    cfg.auto_publish = OptBoolObj::Bool(true);
                }
                Ok(())
            }
            Self::GeoParquet(cfg) => cfg.finalize_target(),
        }
    }

    fn get_unrecognized_keys_with_prefix(&self, prefix: &str) -> UnrecognizedKeys {
        match self {
            Self::Database(cfg) => {
                let mut keys = cfg
                    .unrecognized
                    .keys()
                    .cloned()
                    .map(|k| format!("{prefix}{k}"))
                    .collect::<UnrecognizedKeys>();

                if let Some(ref tables) = cfg.tables {
                    for (k, v) in tables {
                        copy_unrecognized_keys_from_config(
                            &mut keys,
                            &format!("{prefix}tables.{k}."),
                            &v.unrecognized,
                        );
                    }
                }
                if let Some(ref macros) = cfg.macros {
                    for (k, v) in macros {
                        copy_unrecognized_keys_from_config(
                            &mut keys,
                            &format!("{prefix}macros.{k}."),
                            &v.unrecognized,
                        );
                    }
                }
                match &cfg.auto_publish {
                    OptBoolObj::NoValue | OptBoolObj::Bool(_) => {}
                    OptBoolObj::Object(o) => keys.extend(
                        o.get_unrecognized_keys()
                            .iter()
                            .map(|k| format!("{prefix}auto_publish.{k}")),
                    ),
                }
                #[cfg(all(feature = "mlt", feature = "_tiles"))]
                {
                    if let Some(AutoOption::Explicit(mlt_cfg)) = cfg.convert_to_mlt.as_ref() {
                        keys.extend(
                            mlt_cfg
                                .unrecognized_keys()
                                .map(|k| format!("{prefix}convert_to_mlt.{k}")),
                        );
                    }
                    if let Some(AutoOption::Explicit(mvt_cfg)) = cfg.convert_to_mvt.as_ref() {
                        keys.extend(
                            mvt_cfg
                                .unrecognized_keys()
                                .map(|k| format!("{prefix}convert_to_mvt.{k}")),
                        );
                    }
                }
                keys
            }
            Self::GeoParquet(cfg) => {
                let mut keys = cfg
                    .unrecognized
                    .keys()
                    .cloned()
                    .map(|k| format!("{prefix}{k}"))
                    .collect::<UnrecognizedKeys>();
                #[cfg(all(feature = "mlt", feature = "_tiles"))]
                {
                    if let Some(AutoOption::Explicit(mlt_cfg)) = cfg.convert_to_mlt.as_ref() {
                        keys.extend(
                            mlt_cfg
                                .unrecognized_keys()
                                .map(|k| format!("{prefix}convert_to_mlt.{k}")),
                        );
                    }
                    if let Some(AutoOption::Explicit(mvt_cfg)) = cfg.convert_to_mvt.as_ref() {
                        keys.extend(
                            mvt_cfg
                                .unrecognized_keys()
                                .map(|k| format!("{prefix}convert_to_mvt.{k}")),
                        );
                    }
                }
                keys
            }
        }
    }
    /// Validates a single source to ensure no ambiguous keys were swept into `unrecognized`
    pub fn validate(&self) -> Result<(), String> {
        match self {
            DuckDbSourceConfig::Database(db) => {
                if db.unrecognized.contains_key("geoparquet") {
                    return Err(
                        "ambiguous source: contains exactly one database or geoparquet target"
                            .to_string(),
                    );
                }
            }
            DuckDbSourceConfig::GeoParquet(geo) => {
                if geo.unrecognized.contains_key("database") {
                    return Err(
                        "ambiguous source: contains exactly one database or geoparquet target"
                            .to_string(),
                    );
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::path::PathBuf;

    use indoc::indoc;
    use url::Url;

    use super::*;
    use crate::config::primitives::OptOneMany::{Many, One};

    fn finalize(mut cfg: DuckDbConfig) -> DuckDbConfig {
        cfg.finalize().expect("duckdb config should finalize");
        cfg
    }

    #[test]
    fn parses_database_source_with_wrapper_defaults() {
        let cfg = finalize(
            serde_yaml::from_str(indoc! {"
            pool_size: 4
            auto_bounds: quick
            default_srid: 4326
            sources:
              - database: /data/tiles.duckdb
                tables:
                  roads:
                    schema: main
                    table: roads
                    geometry_column: geom
                    srid: 4326
        "})
            .expect("parse duckdb config"),
        );

        assert_eq!(cfg.pool_size, Some(4));
        assert_eq!(cfg.auto_bounds, Some(BoundsCalcType::Quick));
        assert_eq!(cfg.default_srid, Some(4326));
        assert_eq!(cfg.sources.len(), 1);
        match &cfg.sources[0] {
            DuckDbSourceConfig::Database(db) => {
                assert_eq!(db.database, PathBuf::from("/data/tiles.duckdb"));
                assert!(db.tables.is_some());
            }
            DuckDbSourceConfig::GeoParquet(_) => panic!("expected database source"),
        }
    }

    #[test]
    fn parses_geoparquet_into_local_target() {
        let cfg = finalize(
            serde_yaml::from_str(indoc! {"
            sources:
              - geoparquet: /data/buildings.parquet
                layer_id: buildings
        "})
            .expect("parse duckdb config"),
        );

        match &cfg.sources[0] {
            DuckDbSourceConfig::GeoParquet(gp) => {
                assert_eq!(gp.layer_id.as_deref(), Some("buildings"));
                assert_eq!(
                    gp.target(),
                    &super::super::config_geoparquet::GeoParquetTarget::Local(PathBuf::from(
                        "/data/buildings.parquet"
                    ))
                );
            }
            DuckDbSourceConfig::Database(_) => panic!("expected geoparquet source"),
        }
    }

    #[test]
    fn parses_geoparquet_into_remote_target() {
        let cfg = finalize(
            serde_yaml::from_str(indoc! {"
            sources:
              - geoparquet: s3://bucket/roads.parquet
        "})
            .expect("parse duckdb config"),
        );

        match &cfg.sources[0] {
            DuckDbSourceConfig::GeoParquet(gp) => {
                assert!(matches!(
                    gp.target(),
                    super::super::config_geoparquet::GeoParquetTarget::Remote(_)
                ));
                if let super::super::config_geoparquet::GeoParquetTarget::Remote(url) = gp.target()
                {
                    assert_eq!(url, &Url::parse("s3://bucket/roads.parquet").unwrap());
                }
            }
            DuckDbSourceConfig::Database(_) => panic!("expected geoparquet source"),
        }
    }

    #[test]
    fn rejects_ambiguous_source_target() {
        let config = serde_yaml::from_str::<DuckDbConfig>(indoc! {"
            sources:
              - database: /data/tiles.duckdb
                geoparquet: /data/buildings.parquet
        "})
        .expect("semantically invalid configuration");
        let err = config
            .validate()
            .expect_err("ambiguous source should fail validation");
        assert!(err.contains("exactly one"));
    }

    #[test]
    fn effective_pool_size_uses_wrapper_default() {
        let cfg = finalize(
            serde_yaml::from_str(indoc! {"
            sources:
              - database: /data/tiles.duckdb
        "})
            .expect("parse duckdb config"),
        );
        assert_eq!(cfg.effective_pool_size(&cfg.sources[0]), DEFAULT_POOL_SIZE);
    }

    #[test]
    fn parses_sample_config_structure() {
        let cfg = finalize(
            serde_yaml::from_str(indoc! {"
            pool_size: 4
            auto_bounds: quick
            sources:
              - database: /data/tiles.duckdb
                auto_publish:
                  tables:
                    from_schemas: autodetect
                    source_id_format: '{table}'
                    id_columns: [id, gid]
                    extent: 4096
                    buffer: 64
                    clip_geom: true
                  macros:
                    from_schemas: autodetect
                    source_id_format: '{macro}'
                tables:
                  roads:
                    schema: main
                    table: roads
                    geometry_column: geom
                    srid: 4326
                    minzoom: 0
                    maxzoom: 14
                    properties:
                      id: int4
                      name: varchar
                macros:
                  custom:
                    schema: main
                    macro: get_custom_tile
              - geoparquet: /data/buildings.parquet
                layer_id: buildings
                geometry_column: geom
                srid: 4326
                minzoom: 0
                maxzoom: 14
                extent: 4096
                buffer: 64
        "})
            .expect("parse duckdb config"),
        );

        assert_eq!(cfg.pool_size, Some(4));
        assert_eq!(cfg.auto_bounds, Some(BoundsCalcType::Quick));
        assert_eq!(cfg.sources.len(), 2);

        match &cfg.sources[0] {
            DuckDbSourceConfig::Database(db) => {
                assert_eq!(db.database, PathBuf::from("/data/tiles.duckdb"));
                let OptBoolObj::Object(auto_publish) = &db.auto_publish else {
                    panic!("expected auto_publish object");
                };
                let OptBoolObj::Object(tables_cfg) = &auto_publish.tables else {
                    panic!("expected auto_publish.tables object");
                };
                assert_eq!(tables_cfg.from_schemas, One("autodetect".to_string()));
                assert_eq!(tables_cfg.source_id_format.as_deref(), Some("{table}"));
                assert_eq!(
                    tables_cfg.id_columns,
                    Many(vec!["id".to_string(), "gid".to_string()])
                );
                assert_eq!(tables_cfg.extent.map(NonZeroU32::get), Some(4096));
                assert_eq!(tables_cfg.buffer, Some(64));
                assert_eq!(tables_cfg.clip_geom, Some(true));

                let OptBoolObj::Object(macros_cfg) = &auto_publish.macros else {
                    panic!("expected auto_publish.macros object");
                };
                assert_eq!(macros_cfg.from_schemas, One("autodetect".to_string()));
                assert_eq!(macros_cfg.source_id_format.as_deref(), Some("{macro}"));

                let roads = db.tables.as_ref().unwrap().get("roads").unwrap();
                assert_eq!(roads.schema, "main");
                assert_eq!(roads.table, "roads");
                assert_eq!(roads.geometry_column, "geom");
                assert_eq!(roads.srid, 4326);
                assert_eq!(roads.minzoom, Some(0));
                assert_eq!(roads.maxzoom, Some(14));

                let custom = db.macros.as_ref().unwrap().get("custom").unwrap();
                assert_eq!(custom.schema, "main");
                assert_eq!(custom.macro_name, "get_custom_tile");
            }
            DuckDbSourceConfig::GeoParquet(_) => panic!("expected database source"),
        }

        match &cfg.sources[1] {
            DuckDbSourceConfig::GeoParquet(gp) => {
                assert_eq!(gp.layer_id.as_deref(), Some("buildings"));
                assert_eq!(gp.geometry_column.as_deref(), Some("geom"));
                assert_eq!(gp.srid, Some(4326));
                assert_eq!(gp.minzoom, Some(0));
                assert_eq!(gp.maxzoom, Some(14));
                assert_eq!(gp.extent.map(NonZeroU32::get), Some(4096));
                assert_eq!(gp.buffer, Some(64));
            }
            DuckDbSourceConfig::Database(_) => panic!("expected geoparquet source"),
        }
    }

    #[test]
    fn default_impl_yields_empty_sources() {
        assert!(DuckDbConfig::default().sources.is_empty());
    }
}

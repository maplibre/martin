use std::num::NonZeroUsize;
use serde::{Deserialize, Serialize};

use crate::config::args::BoundsCalcType;
use crate::config::file::tiles::duckdb::sources::{
    DuckDbDatabaseEntry, DuckDbSourceDefaults, GeoParquetEntry,
};
use crate::config::file::{
    ConfigFileResult, ConfigurationLivecycleHooks, UnrecognizedKeys, UnrecognizedValues,
};

const DEFAULT_POOL_SIZE: usize = 4;

fn default_pool_size() -> NonZeroUsize {
    NonZeroUsize::new(DEFAULT_POOL_SIZE).expect("default pool size must be non-zero")
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires `&T`"
)]
fn is_default_pool_size(v: &NonZeroUsize) -> bool {
    v.get() == DEFAULT_POOL_SIZE
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires `&T`"
)]
fn is_default_auto_bounds(v: &BoundsCalcType) -> bool {
    *v == BoundsCalcType::default()
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct DuckDbConfig {
    /// Connection pool size used by DuckDB sources unless overridden per-source.
    #[serde(
        default = "default_pool_size",
        skip_serializing_if = "is_default_pool_size"
    )]
    pub pool_size: NonZeroUsize,
    /// Optional DuckDB execution thread count for each connection.
    pub threads: Option<NonZeroUsize>,
    /// Optional DuckDB memory limit in megabytes for each connection.
    pub memory_limit_mb: Option<NonZeroUsize>,
    /// Bounds behavior for auto-generated TileJSON bounds.
    #[serde(default, skip_serializing_if = "is_default_auto_bounds")]
    pub auto_bounds: BoundsCalcType,
    /// Ordered source definitions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<DuckDbSourceEntry>,
    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl Default for DuckDbConfig {
    fn default() -> Self {
        Self {
            pool_size: default_pool_size(),
            threads: None,
            memory_limit_mb: None,
            auto_bounds: BoundsCalcType::default(),
            sources: Vec::new(),
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

impl ConfigurationLivecycleHooks for DuckDbConfig {
    fn finalize(&mut self) -> ConfigFileResult<()> {
        let defaults = DuckDbSourceDefaults {
            pool_size: self.pool_size,
            threads: self.threads,
            memory_limit_mb: self.memory_limit_mb,
            auto_bounds: self.auto_bounds,
        };

        for source in &mut self.sources {
            source.finalize();
            source.apply_defaults(defaults);
        }

        Ok(())
    }

    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        let mut keys: UnrecognizedKeys = self.unrecognized.keys().cloned().collect();
        for (idx, source) in self.sources.iter().enumerate() {
            keys.extend(source.get_unrecognized_keys(&format!("sources[{idx}].")));
        }
        keys
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DuckDbSourceEntry {
    Database(DuckDbDatabaseEntry),
    GeoParquet(GeoParquetEntry),
}

impl DuckDbSourceEntry {
    pub(crate) fn finalize(&mut self) {
        match self {
            Self::Database(v) => v.finalize(),
            Self::GeoParquet(v) => v.finalize(),
        }
    }

    pub(crate) fn apply_defaults(&mut self, defaults: DuckDbSourceDefaults) {
        match self {
            Self::Database(v) => v.settings.apply_defaults(defaults),
            Self::GeoParquet(v) => v.settings.apply_defaults(defaults),
        }
    }

    #[must_use]
    pub(crate) fn get_unrecognized_keys(&self, prefix: &str) -> UnrecognizedKeys {
        let values = match self {
            Self::Database(v) => &v.unrecognized,
            Self::GeoParquet(v) => &v.unrecognized,
        };
        values.keys().map(|k| format!("{prefix}{k}")).collect()
    }
}

#[cfg(feature = "unstable-schemas")]
impl schemars::JsonSchema for DuckDbSourceEntry {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "DuckDbSourceEntry".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let database = generator.subschema_for::<DuckDbDatabaseEntry>();
        let geoparquet = generator.subschema_for::<GeoParquetEntry>();
        schemars::json_schema!({
            "description": "DuckDB source entry: exactly one of `database` or `geoparquet` must be present.",
            "oneOf": [
                database,
                geoparquet,
            ]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_list_may_mix_database_and_geoparquet() {
        let yaml = r#"
pool_size: 4
auto_bounds: quick
sources:
  - database: /data/tiles.duckdb
    auto_publish:
      tables:
        from_schemas: autodetect
  - geoparquet: /data/buildings.parquet
    layer_id: buildings
    geometry_column: geom
    srid: 4326
    minzoom: 0
    maxzoom: 14
    extent: 4096
    buffer: 64
"#;
        let cfg: DuckDbConfig = serde_saphyr::from_str(yaml).expect("duckdb config");

        insta::assert_debug_snapshot!(cfg, @r#"
        DuckDbConfig {
            pool_size: 4,
            threads: None,
            memory_limit_mb: None,
            auto_bounds: Quick,
            sources: [
                Database(
                    DuckDbDatabaseEntry {
                        database: "/data/tiles.duckdb",
                        settings: DuckDbSourceSettings {
                            pool_size: None,
                            threads: None,
                            memory_limit_mb: None,
                            auto_bounds: None,
                        },
                        auto_publish: Some(
                            Object {
                                "tables": Object {
                                    "from_schemas": String("autodetect"),
                                },
                            },
                        ),
                        tables: None,
                        macros: None,
                        unrecognized: {},
                    },
                ),
                GeoParquet(
                    GeoParquetEntry {
                        geoparquet: "/data/buildings.parquet",
                        layer_id: Some(
                            "buildings",
                        ),
                        id_column: None,
                        geometry_column: Some(
                            "geom",
                        ),
                        srid: Some(
                            4326,
                        ),
                        minzoom: Some(
                            0,
                        ),
                        maxzoom: Some(
                            14,
                        ),
                        extent: Some(
                            4096,
                        ),
                        buffer: Some(
                            64,
                        ),
                        clip_geom: None,
                        settings: DuckDbSourceSettings {
                            pool_size: None,
                            threads: None,
                            memory_limit_mb: None,
                            auto_bounds: None,
                        },
                        unrecognized: {},
                    },
                ),
            ],
            unrecognized: {},
        }
        "#);
    }

    #[test]
    fn source_overrides_from_yaml_take_precedence_over_top_level() {
        let yaml = r#"
pool_size: 8
threads: 2
memory_limit_mb: 1024
auto_bounds: quick
sources:
  - geoparquet: /tmp/a.parquet
    pool_size: 3
    memory_limit_mb: 256
    auto_bounds: skip
"#;
        let mut cfg: DuckDbConfig = serde_saphyr::from_str(yaml).expect("duckdb config");
        cfg.finalize().expect("finalize duckdb config");

        insta::assert_debug_snapshot!(cfg, @r#"
        DuckDbConfig {
            pool_size: 8,
            threads: Some(
                2,
            ),
            memory_limit_mb: Some(
                1024,
            ),
            auto_bounds: Quick,
            sources: [
                GeoParquet(
                    GeoParquetEntry {
                        geoparquet: "/tmp/a.parquet",
                        layer_id: None,
                        id_column: None,
                        geometry_column: None,
                        srid: None,
                        minzoom: None,
                        maxzoom: None,
                        extent: None,
                        buffer: None,
                        clip_geom: None,
                        settings: DuckDbSourceSettings {
                            pool_size: Some(
                                3,
                            ),
                            threads: Some(
                                2,
                            ),
                            memory_limit_mb: Some(
                                256,
                            ),
                            auto_bounds: Some(
                                Skip,
                            ),
                        },
                        unrecognized: {},
                    },
                ),
            ],
            unrecognized: {},
        }
        "#);
    }

    #[test]
    fn source_entry_with_both_keys_deserializes_as_database() {
        let yaml = r#"
sources:
  - database: /data/tiles.duckdb
    geoparquet: /data/buildings.parquet
"#;
        let cfg: DuckDbConfig = serde_saphyr::from_str(yaml).expect("duckdb config");

        insta::assert_debug_snapshot!(cfg, @r#"
        DuckDbConfig {
            pool_size: 4,
            threads: None,
            memory_limit_mb: None,
            auto_bounds: Quick,
            sources: [
                Database(
                    DuckDbDatabaseEntry {
                        database: "/data/tiles.duckdb",
                        settings: DuckDbSourceSettings {
                            pool_size: None,
                            threads: None,
                            memory_limit_mb: None,
                            auto_bounds: None,
                        },
                        auto_publish: None,
                        tables: None,
                        macros: None,
                        unrecognized: {
                            "geoparquet": String("/data/buildings.parquet"),
                        },
                    },
                ),
            ],
            unrecognized: {},
        }
        "#);
    }

    #[test]
    fn source_entry_rejects_missing_database_and_geoparquet() {
        let yaml = r#"
sources:
  - layer_id: buildings
    srid: 4326
"#;
        let err = serde_saphyr::from_str::<DuckDbConfig>(yaml).expect_err("missing entry keys");
        assert!(
            err.to_string()
                .contains("data did not match any variant of untagged enum DuckDbSourceEntry")
        );
    }
}

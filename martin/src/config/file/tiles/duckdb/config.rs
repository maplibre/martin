use std::num::NonZeroUsize;

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use serde::Serializer;

use crate::config::args::BoundsCalcType;
use crate::config::file::tiles::duckdb::sources::{DuckDbDatabaseEntry, DuckDbSourceDefaults, GeoParquetEntry};
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
    #[serde(default = "default_pool_size", skip_serializing_if = "is_default_pool_size")]
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

#[derive(Clone, Debug, PartialEq)]
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

impl Serialize for DuckDbSourceEntry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Database(v) => v.serialize(serializer),
            Self::GeoParquet(v) => v.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for DuckDbSourceEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| de::Error::custom("duckdb source entry must be a YAML mapping"))?;

        let has_database = object.contains_key("database");
        let has_geoparquet = object.contains_key("geoparquet");

        match (has_database, has_geoparquet) {
            (true, false) => serde_json::from_value::<DuckDbDatabaseEntry>(value)
                .map(Self::Database)
                .map_err(|e| de::Error::custom(format!("invalid database source entry: {e}"))),
            (false, true) => serde_json::from_value::<GeoParquetEntry>(value)
                .map(Self::GeoParquet)
                .map_err(|e| de::Error::custom(format!("invalid geoparquet source entry: {e}"))),
            (false, false) => Err(de::Error::custom(
                "duckdb source entry must contain exactly one of `database` or `geoparquet`",
            )),
            (true, true) => Err(de::Error::custom(
                "duckdb source entry cannot contain both `database` and `geoparquet`",
            )),
        }
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
        assert_eq!(cfg.sources.len(), 2);

        let db = cfg.sources.iter().find_map(|source| match source {
            DuckDbSourceEntry::Database(db) => Some(db),
            DuckDbSourceEntry::GeoParquet(_) => None,
        });
        let gpq = cfg.sources.iter().find_map(|source| match source {
            DuckDbSourceEntry::GeoParquet(gpq) => Some(gpq),
            DuckDbSourceEntry::Database(_) => None,
        });

        let Some(db) = db else {
            panic!("expected one database source entry");
        };
        let Some(gpq) = gpq else {
            panic!("expected one geoparquet source entry");
        };

        assert_eq!(db.database, std::path::PathBuf::from("/data/tiles.duckdb"));
        assert_eq!(
            gpq.geoparquet,
            std::path::PathBuf::from("/data/buildings.parquet")
        );
        assert_eq!(gpq.layer_id.as_deref(), Some("buildings"));
        assert_eq!(gpq.geometry_column.as_deref(), Some("geom"));
        assert_eq!(gpq.srid, Some(4326));
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

        let DuckDbSourceEntry::GeoParquet(source) = &cfg.sources[0] else {
            panic!("expected one geoparquet source");
        };

        assert_eq!(source.settings.pool_size, NonZeroUsize::new(3));
        assert_eq!(source.settings.threads, NonZeroUsize::new(2));
        assert_eq!(source.settings.memory_limit_mb, NonZeroUsize::new(256));
        assert_eq!(source.settings.auto_bounds, Some(BoundsCalcType::Skip));
    }

    #[test]
    fn source_entry_rejects_both_database_and_geoparquet() {
        let yaml = r#"
sources:
  - database: /data/tiles.duckdb
    geoparquet: /data/buildings.parquet
"#;
        let err = serde_saphyr::from_str::<DuckDbConfig>(yaml).expect_err("mixed entry keys");
        assert!(
            err.to_string()
                .contains("cannot contain both `database` and `geoparquet`")
        );
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
                .contains("must contain exactly one of `database` or `geoparquet`")
        );
    }
}

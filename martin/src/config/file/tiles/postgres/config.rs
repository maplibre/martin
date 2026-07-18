use std::num::{NonZeroU32, NonZeroUsize};
use std::ops::Add as _;
use std::time::Duration;

use futures::future::join_all;
use futures::pin_mut;
use martin_tile_utils::TileInfo;
use serde::{Deserialize, Serialize};
use tilejson::TileJSON;
use tokio::time::timeout;
use tracing::{info, warn};

use super::{FuncInfoSources, TableInfoSources};
use crate::config::args::{BoundsCalcType, DEFAULT_BOUNDS_TIMEOUT};
use crate::config::file::postgres::{PostgresAutoDiscoveryBuilder, SourceSpec};
use crate::config::file::{
    CachePolicy, ConfigFileError, ConfigFileResult, ConfigurationLivecycleHooks, ResolutionResult,
    TileSourceWarning, UnrecognizedKeys, UnrecognizedValues, copy_unrecognized_keys_from_config,
};
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::{MltProcessConfig, MvtProcessConfig};
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::primitives::AutoOption;
use crate::config::primitives::{IdResolver, OptBoolObj, OptOneMany};

/// Default interval at which the [`PostgresReloader`](crate::config::file::reload::postgres::PostgresReloader)
/// re-runs catalog discovery to pick up new, changed, or dropped tables and functions at runtime.
pub const DEFAULT_RELOAD_INTERVAL: Duration = Duration::from_mins(10);

fn default_reload_interval() -> Duration {
    DEFAULT_RELOAD_INTERVAL
}

fn is_default_reload_interval(v: &Duration) -> bool {
    *v == DEFAULT_RELOAD_INTERVAL
}

pub trait PostgresInfo {
    fn format_id(&self) -> String;
    fn to_tilejson(&self, source_id: String) -> TileJSON;
    /// Return the tile format and encoding for this source.
    fn tile_info(&self) -> TileInfo;
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct PostgresSslCerts {
    /// Same as `PGSSLCERT` for `psql`
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"./postgresql.crt"))]
    pub ssl_cert: Option<std::path::PathBuf>,
    /// Same as `PGSSLKEY` for `psql`
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"./postgresql.key"))]
    pub ssl_key: Option<std::path::PathBuf>,
    /// Same as `PGSSLROOTCERT` for `psql`
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"./root.crt"))]
    pub ssl_root_cert: Option<std::path::PathBuf>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct PostgresConfig {
    /// Database connection string.
    ///
    /// You can use environment variables too, for example:
    /// `connection_string: $DATABASE_URL`
    /// `connection_string: ${DATABASE_URL:-postgres://postgres@localhost/db}`
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(example = &"postgres://postgres@localhost:5432/db")
    )]
    pub connection_string: Option<String>,
    #[serde(flatten)]
    pub ssl_certificates: PostgresSslCerts,
    /// If a spatial table has SRID 0, then this SRID will be used as a fallback
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &4326i32))]
    pub default_srid: Option<i32>,
    /// Specify how bounds should be computed for the spatial PG tables \[default: quick\]
    ///
    /// Options:
    /// - `calc` compute table geometry bounds on startup.
    /// - `quick` same as 'calc', but the calculation will be aborted after 5 seconds.
    /// - `skip` does not compute table geometry bounds on startup.
    pub auto_bounds: Option<BoundsCalcType>,
    /// Limit the number of geo features per tile.
    ///
    /// If the source table has more features than set here, they will not be
    /// included in the tile and the result will look "cut off"/incomplete.
    /// This feature allows you to put a maximum latency bound on tiles with an
    /// extreme amount of detail at the cost of not returning all data.
    /// It is sensible to set this limit if you have user generated/untrusted
    /// geodata, e.g. a lot of data points at [Null Island](https://en.wikipedia.org/wiki/Null_Island).
    ///
    /// either a positive integer, or null=unlimited (default)
    pub max_feature_count: Option<usize>,
    /// Maximum Postgres connections pool size \[default: 20\]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &20usize))]
    pub pool_size: Option<NonZeroUsize>,
    /// How often the `PostgresReloader` re-runs catalog discovery to publish new tables and
    /// functions, update changed ones, and drop removed ones at runtime, without a restart.
    ///
    /// Supports human-readable formats: "10m", "1h", "30s".
    /// Defaults to "10m". Set to "0s" to disable runtime reloading.
    #[serde(
        default = "default_reload_interval",
        skip_serializing_if = "is_default_reload_interval",
        with = "humantime_serde"
    )]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "String", example = &"10m")
    )]
    pub reload_interval: Duration,
    /// Enable automatic discovery of tables and functions. \[default: null\]
    ///
    /// Options:
    /// - `true`: run automatic discovery (`true` may be omitted if further configuration is provided)
    /// - `false`: disable automatic discovery
    /// - null: run automatic discovery if `postgres.tables` is null and `postgres.functions` is null
    #[serde(default, skip_serializing_if = "OptBoolObj::is_none")]
    pub auto_publish: OptBoolObj<PostgresCfgPublish>,
    /// Associative arrays of table sources
    pub tables: Option<TableInfoSources>,
    /// Associative arrays of function sources
    pub functions: Option<FuncInfoSources>,

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

/// Default connection pool size.
pub const DEFAULT_POOL_SIZE: NonZeroUsize =
    NonZeroUsize::new(20).expect("default pool size is non-zero");

impl Default for PostgresConfig {
    // Hand-implemented (not derived) so `..Default::default()` yields a 10-minute
    // `reload_interval` rather than `Duration::ZERO`; the config-equality tests rely on this.
    fn default() -> Self {
        Self {
            connection_string: None,
            ssl_certificates: PostgresSslCerts::default(),
            default_srid: None,
            auto_bounds: None,
            max_feature_count: None,
            pool_size: None,
            reload_interval: DEFAULT_RELOAD_INTERVAL,
            auto_publish: OptBoolObj::default(),
            tables: None,
            functions: None,
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            convert_to_mlt: None,
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            convert_to_mvt: None,
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct PostgresCfgPublish {
    /// Optionally limit to just these schemas
    #[serde(alias = "from_schema")]
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub from_schemas: OptOneMany<String>,
    /// Here we enable both tables and functions auto discovery.
    /// You can also enable just one of them by not mentioning the other, or
    /// setting it to false. Setting one to true disables the other one as well.
    /// E.g. `tables: false` enables just the functions auto-discovery.
    #[serde(default, skip_serializing_if = "OptBoolObj::is_none")]
    pub tables: OptBoolObj<PostgresCfgPublishTables>,
    #[serde(default, skip_serializing_if = "OptBoolObj::is_none")]
    pub functions: OptBoolObj<PostgresCfgPublishFuncs>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigurationLivecycleHooks for PostgresCfgPublish {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        let mut keys = self
            .unrecognized
            .keys()
            .cloned()
            .collect::<UnrecognizedKeys>();
        match &self.functions {
            OptBoolObj::NoValue | OptBoolObj::Bool(_) => {}
            OptBoolObj::Object(o) => keys.extend(
                o.get_unrecognized_keys()
                    .iter()
                    .map(|k| format!("functions.{k}")),
            ),
        }
        match &self.tables {
            OptBoolObj::NoValue | OptBoolObj::Bool(_) => {}
            OptBoolObj::Object(o) => keys.extend(
                o.get_unrecognized_keys()
                    .iter()
                    .map(|k| format!("tables.{k}")),
            ),
        }
        keys
    }
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct PostgresCfgPublishTables {
    /// Add more schemas to the ones listed above
    #[serde(alias = "from_schema")]
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub from_schemas: OptOneMany<String>,
    /// Optionally set how source ID should be generated based on the table's name,
    /// schema, and geometry column
    #[serde(alias = "id_format")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(example = &"table.{schema}.{table}.{column}")
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

impl ConfigurationLivecycleHooks for PostgresCfgPublishTables {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct PostgresCfgPublishFuncs {
    /// Optionally limit to just these schemas
    #[serde(alias = "from_schema")]
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub from_schemas: OptOneMany<String>,
    /// Optionally set how source ID should be generated based on the function's
    /// name and schema
    #[serde(alias = "id_format")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(example = &"{schema}.{function}")
    )]
    pub source_id_format: Option<String>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigurationLivecycleHooks for PostgresCfgPublishFuncs {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}

impl PostgresConfig {
    pub async fn resolve(
        &mut self,
        id_resolver: IdResolver,
        default_cache: CachePolicy,
    ) -> ResolutionResult {
        let pg = PostgresAutoDiscoveryBuilder::new(self, id_resolver, default_cache).await?;

        let (specs, mut warnings) = pg.discover().await?;

        // Build each source concurrently, warning if the bounds work drags on.
        let pg_ref = &pg;
        let pending = specs.into_iter().map(move |(id, spec)| async move {
            (id.clone(), pg_ref.instantiate(&id, spec).await)
        });
        let instantiated = on_slow(
            join_all(pending),
            // warn only if default bounds timeout has already passed
            DEFAULT_BOUNDS_TIMEOUT.add(Duration::from_secs(1)),
            || {
                if pg.auto_bounds() == BoundsCalcType::Skip {
                    warn!(
                        "Discovering tables in PostgreSQL database '{}' is taking too long. Bounds calculation is already disabled. You may need to tune your database.",
                        pg.get_id()
                    );
                } else {
                    warn!(
                        "Discovering tables in PostgreSQL database '{}' is taking too long. Make sure your table geo columns have a GIS index, or use '--auto-bounds skip' CLI/config to skip bbox calculation.",
                        pg.get_id()
                    );
                }
            },
        )
        .await;

        // Write back the resolved tables/functions for `--save-config`, collect the live sources, and surface per-source failures as warnings.
        let mut sources = Vec::new();
        let mut tables = TableInfoSources::new();
        let mut functions = FuncInfoSources::new();
        for (id, result) in instantiated {
            match result {
                Ok((source, SourceSpec::Table(info))) => {
                    let kind = match info.relkind {
                        Some('v') => "view",
                        Some('m') => "materialized view",
                        _ => "table",
                    };
                    info!(
                        source.id = %id,
                        source.kind = kind,
                        schema = %info.schema,
                        table = %info.table,
                        geometry_column = %info.geometry_column,
                        geometry_type = info.geometry_type.as_deref().unwrap_or("unknown"),
                        srid = info.srid,
                        id_column = info.id_column.as_deref().unwrap_or("none"),
                        "Published source"
                    );
                    sources.push(source);
                    tables.insert(id, info);
                }
                Ok((source, SourceSpec::Function(info, sql))) => {
                    info!(
                        source.id = %id,
                        source.kind = "function",
                        schema = %info.schema,
                        function = %info.function,
                        function.signature = %sql.signature,
                        "Published source"
                    );
                    sources.push(source);
                    functions.insert(id, info);
                }
                Err(error) => warnings.push(TileSourceWarning::SourceError {
                    source_id: id,
                    error: error.to_string(),
                }),
            }
        }

        self.tables = Some(tables);
        self.functions = Some(functions);
        Ok((sources, warnings))
    }
}

impl ConfigurationLivecycleHooks for PostgresConfig {
    fn finalize(&mut self) -> ConfigFileResult<()> {
        if self.tables.is_none() && self.functions.is_none() && self.auto_publish.is_none() {
            self.auto_publish = OptBoolObj::Bool(true);
        }

        if self.connection_string.is_none() {
            return Err(ConfigFileError::PostgresConnectionStringMissing);
        }

        Ok(())
    }

    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        let mut keys = self
            .unrecognized
            .keys()
            .cloned()
            .collect::<UnrecognizedKeys>();

        if let Some(ref ts) = self.tables {
            for (k, v) in ts {
                copy_unrecognized_keys_from_config(
                    &mut keys,
                    &format!("tables.{k}."),
                    &v.unrecognized,
                );
            }
        }
        if let Some(ref fs) = self.functions {
            for (k, v) in fs {
                copy_unrecognized_keys_from_config(
                    &mut keys,
                    &format!("functions.{k}."),
                    &v.unrecognized,
                );
            }
        }

        keys.extend(
            self.ssl_certificates
                .unrecognized
                .keys()
                .map(|k| format!("ssl_certificates.{k}")),
        );

        match &self.auto_publish {
            OptBoolObj::NoValue | OptBoolObj::Bool(_) => {}
            OptBoolObj::Object(o) => keys.extend(
                o.get_unrecognized_keys()
                    .iter()
                    .map(|k| format!("auto_publish.{k}"))
                    .collect::<UnrecognizedKeys>(),
            ),
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

async fn on_slow<T, S: FnOnce()>(
    future: impl Future<Output = T>,
    duration: Duration,
    fn_on_slow: S,
) -> T {
    pin_mut!(future);
    if let Ok(result) = timeout(duration, &mut future).await {
        result
    } else {
        fn_on_slow();
        future.await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::path::Path;

    use indoc::indoc;
    use tilejson::Bounds;

    use super::*;
    use crate::config::file::postgres::{FunctionInfo, TableInfo};
    use crate::config::file::{Config, parse_config};
    use crate::config::primitives::OptOneMany::{Many, One};
    use crate::config::test_helpers::render_finalize_failure;

    pub fn parse_cfg(yaml: &str) -> Config {
        parse_config(yaml, &HashMap::new(), Path::new("<test>")).unwrap()
    }

    pub fn assert_config(yaml: &str, expected: &Config) {
        let mut config = parse_cfg(yaml);
        let res = futures::executor::block_on(config.finalize()).unwrap();
        assert!(res.is_empty(), "unrecognized config: {res:?}");
        assert_eq!(&config, expected);
    }

    #[test]
    fn finalize_postgres_missing_connection_string() {
        insta::assert_snapshot!(
            render_finalize_failure(indoc! {"
                postgres:
                  pool_size: 5
            "}),
            @"A postgres connection string must be provided"
        );
    }

    #[test]
    fn reload_interval_defaults_to_ten_minutes() {
        let cfg: PostgresConfig = serde_saphyr::from_str(indoc! {"
            connection_string: 'postgres://postgres@localhost/db'
        "})
        .unwrap();
        assert_eq!(cfg.reload_interval, DEFAULT_RELOAD_INTERVAL);
        assert_eq!(DEFAULT_RELOAD_INTERVAL, Duration::from_mins(10));
    }

    #[test]
    fn default_impl_yields_ten_minute_reload_interval() {
        // `..Default::default()` must yield 10m so the config-equality tests below still hold.
        assert_eq!(
            PostgresConfig::default().reload_interval,
            DEFAULT_RELOAD_INTERVAL
        );
    }

    #[test]
    fn reload_interval_zero_disables_polling() {
        let cfg: PostgresConfig = serde_saphyr::from_str(indoc! {"
            connection_string: 'postgres://postgres@localhost/db'
            reload_interval: 0s
        "})
        .unwrap();
        assert_eq!(cfg.reload_interval, Duration::ZERO);
    }

    #[test]
    fn parse_pg_one() {
        assert_config(
            indoc! {"
            postgres:
              connection_string: 'postgresql://postgres@localhost/db'
        "},
            &Config {
                postgres: One(PostgresConfig {
                    connection_string: Some("postgresql://postgres@localhost/db".to_string()),
                    auto_publish: OptBoolObj::Bool(true),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
    }

    #[test]
    fn parse_pg_two() {
        assert_config(
            indoc! {"
            postgres:
              - connection_string: 'postgres://postgres@localhost:5432/db'
              - connection_string: 'postgresql://postgres@localhost:5433/db'
        "},
            &Config {
                postgres: Many(vec![
                    PostgresConfig {
                        connection_string: Some(
                            "postgres://postgres@localhost:5432/db".to_string(),
                        ),
                        auto_publish: OptBoolObj::Bool(true),
                        ..Default::default()
                    },
                    PostgresConfig {
                        connection_string: Some(
                            "postgresql://postgres@localhost:5433/db".to_string(),
                        ),
                        auto_publish: OptBoolObj::Bool(true),
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            },
        );
    }

    #[test]
    fn parse_pg_config() {
        assert_config(
            indoc! {"
            postgres:
              connection_string: 'postgres://postgres@localhost:5432/db'
              default_srid: 4326
              pool_size: 20
              max_feature_count: 100

              tables:
                table_source:
                  schema: public
                  table: table_source
                  srid: 4326
                  geometry_column: geom
                  id_column: ~
                  minzoom: 0
                  maxzoom: 30
                  bounds: [-180.0, -90.0, 180.0, 90.0]
                  extent: 2048
                  buffer: 10
                  clip_geom: false
                  geometry_type: GEOMETRY
                  properties:
                    gid: int4

              functions:
                function_zxy_query:
                  schema: public
                  function: function_zxy_query
                  minzoom: 0
                  maxzoom: 30
                  bounds: [-180.0, -90.0, 180.0, 90.0]
        "},
            &Config {
                postgres: One(PostgresConfig {
                    connection_string: Some("postgres://postgres@localhost:5432/db".to_string()),
                    default_srid: Some(4326),
                    pool_size: NonZeroUsize::new(20),
                    max_feature_count: Some(100),
                    tables: Some(BTreeMap::from([(
                        "table_source".to_string(),
                        TableInfo {
                            schema: "public".to_string(),
                            table: "table_source".to_string(),
                            srid: 4326,
                            geometry_column: "geom".to_string(),
                            minzoom: Some(0),
                            maxzoom: Some(30),
                            bounds: Some([-180, -90, 180, 90].into()),
                            extent: NonZeroU32::new(2048),
                            buffer: Some(10),
                            clip_geom: Some(false),
                            geometry_type: Some("GEOMETRY".to_string()),
                            properties: Some(BTreeMap::from([(
                                "gid".to_string(),
                                "int4".to_string(),
                            )])),
                            ..Default::default()
                        },
                    )])),
                    functions: Some(BTreeMap::from([(
                        "function_zxy_query".to_string(),
                        FunctionInfo::new_extended(
                            "public".to_string(),
                            "function_zxy_query".to_string(),
                            0,
                            30,
                            Bounds::MAX,
                        ),
                    )])),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
    }

    #[test]
    fn reject_zero_extent() {
        let yaml = indoc! {"
            schema: public
            table: table_source
            srid: 4326
            geometry_column: geom
            extent: 0
        "};
        let err = serde_saphyr::from_str::<TableInfo>(yaml)
            .expect_err("extent: 0 must be rejected by NonZeroU32");
        let msg = err.to_string();
        assert!(
            msg.contains("extent") || msg.contains("zero") || msg.contains("nonzero"),
            "unexpected error message: {msg}"
        );
    }
}

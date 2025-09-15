use std::ops::Add;
use std::time::Duration;

use futures::future::try_join;
use futures::pin_mut;
use log::warn;
use martin_core::config::{OptBoolObj, OptOneMany};
use martin_core::tiles::BoxedSource;
use martin_core::tiles::postgres::{PgError, PgResult};
use serde::{Deserialize, Serialize};
use tilejson::TileJSON;
use tokio::time::timeout;

use super::{FuncInfoSources, TableInfoSources};
use crate::MartinResult;
use crate::config::args::{BoundsCalcType, DEFAULT_BOUNDS_TIMEOUT};
use crate::config::file::{
    ConfigExtras, UnrecognizedKeys, UnrecognizedValues, copy_unrecognized_keys_from_config,
};
use crate::pg::builder::PgBuilder;
use crate::utils::IdResolver;

pub trait PgInfo {
    fn format_id(&self) -> String;
    fn to_tilejson(&self, source_id: String) -> TileJSON;
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PgSslCerts {
    /// Same as PGSSLCERT
    /// ([docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLCERT))
    pub ssl_cert: Option<std::path::PathBuf>,
    /// Same as PGSSLKEY
    /// ([docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLKEY))
    pub ssl_key: Option<std::path::PathBuf>,
    /// Same as PGSSLROOTCERT
    /// ([docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLROOTCERT))
    pub ssl_root_cert: Option<std::path::PathBuf>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PgConfig {
    /// Database connection string
    pub connection_string: Option<String>,
    #[serde(flatten)]
    pub ssl_certificates: PgSslCerts,
    /// If a spatial table has SRID 0, then this SRID will be used as a fallback
    pub default_srid: Option<i32>,
    /// Specify how bounds should be computed for the spatial PG tables
    pub auto_bounds: Option<BoundsCalcType>,
    /// Limit the number of geo features per tile.
    ///
    /// If the source table has more features than set here, they will not be included in the tile and the result will look "cut off"/incomplete.
    /// This feature allows to put a maximum latency bound on tiles with extreme amount of detail at the cost of not returning all data.
    /// It is sensible to set this limit if you have user generated/untrusted geodata, e.g. a lot of data points at [Null Island](https://en.wikipedia.org/wiki/Null_Island).
    ///
    /// Can be either a positive integer or unlimited if omitted.
    pub max_feature_count: Option<usize>,
    /// Maximum Postgres connections pool size [DEFAULT: 20]
    #[serde(default = "default_pool_size")]
    pub pool_size: usize,
    /// Enable/disable/configure automatic discovery of tables and functions.
    ///
    /// You may set this to `OptBoolObj::Bool(false)` to disable.
    #[serde(default, skip_serializing_if = "OptBoolObj::is_none")]
    pub auto_publish: OptBoolObj<PgCfgPublish>,
    /// Associative arrays of table sources
    pub tables: Option<TableInfoSources>,
    /// Associative arrays of function sources
    pub functions: Option<FuncInfoSources>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

/// Default connection pool size.
pub const POOL_SIZE_DEFAULT: usize = 20;

/// Default connection pool size.
const fn default_pool_size() -> usize {
    POOL_SIZE_DEFAULT // serde only allows functions
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PgCfgPublish {
    #[serde(alias = "from_schema")]
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub from_schemas: OptOneMany<String>,
    #[serde(default, skip_serializing_if = "OptBoolObj::is_none")]
    pub tables: OptBoolObj<PgCfgPublishTables>,
    #[serde(default, skip_serializing_if = "OptBoolObj::is_none")]
    pub functions: OptBoolObj<PgCfgPublishFuncs>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigExtras for PgCfgPublish {
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
                    .map(|k| format!("functions.{k}"))
                    .collect::<UnrecognizedKeys>(),
            ),
        }
        match &self.tables {
            OptBoolObj::NoValue | OptBoolObj::Bool(_) => {}
            OptBoolObj::Object(o) => keys.extend(
                o.get_unrecognized_keys()
                    .iter()
                    .map(|k| format!("tables.{k}"))
                    .collect::<UnrecognizedKeys>(),
            ),
        }
        keys
    }
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PgCfgPublishTables {
    #[serde(alias = "from_schema")]
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub from_schemas: OptOneMany<String>,
    #[serde(alias = "id_format")]
    pub source_id_format: Option<String>,
    /// A table column to use as the feature ID
    /// If a table has no column with this name, `id_column` will not be set for that table.
    /// If a list of strings is given, the first found column will be treated as a feature ID.
    #[serde(alias = "id_column")]
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub id_columns: OptOneMany<String>,
    pub clip_geom: Option<bool>,
    pub buffer: Option<u32>,
    pub extent: Option<u32>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigExtras for PgCfgPublishTables {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PgCfgPublishFuncs {
    #[serde(alias = "from_schema")]
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub from_schemas: OptOneMany<String>,
    #[serde(alias = "id_format")]
    pub source_id_format: Option<String>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigExtras for PgCfgPublishFuncs {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}

impl PgConfig {
    /// Validate if all settings are valid
    pub fn validate(&self) -> PgResult<()> {
        if self.pool_size < 1 {
            return Err(PgError::ConfigError(
                "pool_size must be greater than or equal to 1.",
            ));
        }
        if self.connection_string.is_none() {
            return Err(PgError::ConfigError(
                "A connection string must be provided.",
            ));
        }

        Ok(())
    }

    pub fn finalize(&mut self, prefix: &str) -> PgResult<UnrecognizedKeys> {
        let mut res = UnrecognizedKeys::new();
        if let Some(ref ts) = self.tables {
            for (k, v) in ts {
                copy_unrecognized_keys_from_config(
                    &mut res,
                    &format!("tables.{k}."),
                    &v.unrecognized,
                );
            }
        }
        if let Some(ref fs) = self.functions {
            for (k, v) in fs {
                copy_unrecognized_keys_from_config(
                    &mut res,
                    &format!("functions.{k}."),
                    &v.unrecognized,
                );
            }
        }
        if self.tables.is_none() && self.functions.is_none() && self.auto_publish.is_none() {
            self.auto_publish = OptBoolObj::Bool(true);
        }

        self.validate()?;
        Ok(self
            .get_unrecognized_keys()
            .iter()
            .map(|k| format!("{prefix}{k}"))
            .collect())
    }

    pub async fn resolve(&mut self, id_resolver: IdResolver) -> MartinResult<Vec<BoxedSource>> {
        let pg = PgBuilder::new(self, id_resolver).await?;
        let inst_tables = on_slow(
            pg.instantiate_tables(),
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
        );
        let ((mut tables, tbl_info), (funcs, func_info)) =
            try_join(inst_tables, pg.instantiate_functions()).await?;

        self.tables = Some(tbl_info);
        self.functions = Some(func_info);
        tables.extend(funcs);
        Ok(tables)
    }
}

impl ConfigExtras for PgConfig {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        let mut res = self
            .unrecognized
            .keys()
            .cloned()
            .collect::<UnrecognizedKeys>();

        if let Some(ref ts) = self.tables {
            for (k, v) in ts {
                copy_unrecognized_keys_from_config(
                    &mut res,
                    &format!("tables.{k}."),
                    &v.unrecognized,
                );
            }
        }
        if let Some(ref fs) = self.functions {
            for (k, v) in fs {
                copy_unrecognized_keys_from_config(
                    &mut res,
                    &format!("functions.{k}."),
                    &v.unrecognized,
                );
            }
        }

        res.extend(
            self.ssl_certificates
                .unrecognized
                .keys()
                .map(|k| format!("ssl_certificates.{k}")),
        );

        match &self.auto_publish {
            OptBoolObj::NoValue | OptBoolObj::Bool(_) => {}
            OptBoolObj::Object(o) => res.extend(
                o.get_unrecognized_keys()
                    .iter()
                    .map(|k| format!("auto_publish.{k}"))
                    .collect::<UnrecognizedKeys>(),
            ),
        }

        res
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
    use std::collections::BTreeMap;
    use std::path::Path;

    use indoc::indoc;
    use martin_core::config::OptOneMany::{Many, One};
    use martin_core::config::env::FauxEnv;
    use tilejson::Bounds;

    use super::*;
    use crate::config::file::pg::{FunctionInfo, TableInfo};
    use crate::config::file::{Config, parse_config};

    pub fn parse_cfg(yaml: &str) -> Config {
        parse_config(yaml, &FauxEnv::default(), Path::new("<test>")).unwrap()
    }

    pub fn assert_config(yaml: &str, expected: &Config) {
        let mut config = parse_cfg(yaml);
        let res = config.finalize().unwrap();
        assert!(res.is_empty(), "unrecognized config: {res:?}");
        assert_eq!(&config, expected);
    }

    #[test]
    fn parse_pg_one() {
        assert_config(
            indoc! {"
            postgres:
              connection_string: 'postgresql://postgres@localhost/db'
        "},
            &Config {
                postgres: One(PgConfig {
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
                    PgConfig {
                        connection_string: Some(
                            "postgres://postgres@localhost:5432/db".to_string(),
                        ),
                        auto_publish: OptBoolObj::Bool(true),
                        ..Default::default()
                    },
                    PgConfig {
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
                postgres: One(PgConfig {
                    connection_string: Some("postgres://postgres@localhost:5432/db".to_string()),
                    default_srid: Some(4326),
                    pool_size: 20,
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
                            extent: Some(2048),
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
}

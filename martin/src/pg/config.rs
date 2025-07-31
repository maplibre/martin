use std::ops::Add;
use std::time::Duration;

use futures::future::try_join;
use log::warn;
use serde::{Deserialize, Serialize};
use tilejson::TileJSON;

use crate::MartinResult;
use crate::args::{BoundsCalcType, DEFAULT_BOUNDS_TIMEOUT};
use crate::config::{UnrecognizedKeys, UnrecognizedValues};
use crate::file_config::ConfigExtras;
use crate::pg::builder::PgBuilder;
use crate::pg::config_function::FuncInfoSources;
use crate::pg::config_table::TableInfoSources;
use crate::pg::utils::on_slow;
use crate::pg::{PgError, PgResult};
use crate::source::TileInfoSources;
use crate::utils::{IdResolver, OptBoolObj, OptOneMany};

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
    pub pool_size: Option<usize>,
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
        keys.extend(
            self.functions
                .get_unrecognized_keys()
                .iter()
                .map(|k| format!("functions.{k}"))
                .collect::<UnrecognizedKeys>(),
        );
        keys.extend(
            self.tables
                .get_unrecognized_keys()
                .iter()
                .map(|k| format!("tables.{k}"))
                .collect::<UnrecognizedKeys>(),
        );
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
    /// Apply defaults to the config, and validate if there is a connection string
    pub fn validate(&self) -> PgResult<()> {
        if let Some(pool_size) = self.pool_size {
            if pool_size < 1 {
                return Err(PgError::ConfigError(
                    "pool_size must be greater than or equal to 1.",
                ));
            }
        }
        if self.connection_string.is_none() {
            return Err(PgError::ConfigError(
                "A connection string must be provided.",
            ));
        }

        Ok(())
    }

    pub fn finalize(&mut self, prefix: &'static str) -> PgResult<UnrecognizedKeys> {
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

    pub async fn resolve(&mut self, id_resolver: IdResolver) -> MartinResult<TileInfoSources> {
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
            for (table_key, v) in ts {
                res.extend(
                    v.unrecognized
                        .keys()
                        .map(|unrecognised_key| format!("tables.{table_key}.{unrecognised_key}")),
                );
            }
        }
        if let Some(ref fs) = self.functions {
            for (function_key, v) in fs {
                res.extend(v.unrecognized.keys().map(|unrecognised_key| {
                    format!("functions.{function_key}.{unrecognised_key}")
                }));
            }
        }

        res.extend(
            self.ssl_certificates
                .unrecognized
                .keys()
                .map(|k| format!("ssl_certificates.{k}")),
        );

        res.extend(
            self.auto_publish
                .get_unrecognized_keys()
                .iter()
                .map(|k| format!("auto_publish.{k}")),
        );

        res
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use indoc::indoc;
    use tilejson::Bounds;

    use super::*;
    use crate::config::Config;
    use crate::config::tests::assert_config;
    use crate::pg::config_function::FunctionInfo;
    use crate::pg::config_table::TableInfo;
    use crate::tests::some;
    use crate::utils::OptOneMany::{Many, One};

    #[test]
    fn parse_pg_one() {
        assert_config(
            indoc! {"
            postgres:
              connection_string: 'postgresql://postgres@localhost/db'
        "},
            &Config {
                postgres: One(PgConfig {
                    connection_string: some("postgresql://postgres@localhost/db"),
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
                        connection_string: some("postgres://postgres@localhost:5432/db"),
                        auto_publish: OptBoolObj::Bool(true),
                        ..Default::default()
                    },
                    PgConfig {
                        connection_string: some("postgresql://postgres@localhost:5433/db"),
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
                    connection_string: some("postgres://postgres@localhost:5432/db"),
                    default_srid: Some(4326),
                    pool_size: Some(20),
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
                            geometry_type: some("GEOMETRY"),
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

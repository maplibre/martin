use std::ops::Add;
use std::time::Duration;

use futures::future::try_join;
use log::warn;
use serde::{Deserialize, Serialize};
use tilejson::TileJSON;

use crate::args::{BoundsCalcType, DEFAULT_BOUNDS_TIMEOUT};
use crate::config::{copy_unrecognized_config, UnrecognizedValues};
use crate::pg::config_function::FuncInfoSources;
use crate::pg::config_table::TableInfoSources;
use crate::pg::configurator::PgBuilder;
use crate::pg::utils::on_slow;
use crate::pg::PgResult;
use crate::source::TileInfoSources;
use crate::utils::{IdResolver, OptBoolObj, OptOneMany};
use crate::MartinResult;

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
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PgConfig {
    pub connection_string: Option<String>,
    #[serde(flatten)]
    pub ssl_certificates: PgSslCerts,
    pub default_srid: Option<i32>,
    pub auto_bounds: Option<BoundsCalcType>,
    pub max_feature_count: Option<usize>,
    pub pool_size: Option<usize>,
    #[serde(default, skip_serializing_if = "OptBoolObj::is_none")]
    pub auto_publish: OptBoolObj<PgCfgPublish>,
    pub tables: Option<TableInfoSources>,
    pub functions: Option<FuncInfoSources>,
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
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PgCfgPublishFuncs {
    #[serde(alias = "from_schema")]
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub from_schemas: OptOneMany<String>,
    #[serde(alias = "id_format")]
    pub source_id_format: Option<String>,
}

impl PgConfig {
    /// Apply defaults to the config, and validate if there is a connection string
    pub fn finalize(&mut self) -> PgResult<UnrecognizedValues> {
        let mut res = UnrecognizedValues::new();
        if let Some(ref ts) = self.tables {
            for (k, v) in ts {
                copy_unrecognized_config(&mut res, &format!("tables.{k}."), &v.unrecognized);
            }
        }
        if let Some(ref fs) = self.functions {
            for (k, v) in fs {
                copy_unrecognized_config(&mut res, &format!("functions.{k}."), &v.unrecognized);
            }
        }
        if self.tables.is_none() && self.functions.is_none() && self.auto_publish.is_none() {
            self.auto_publish = OptBoolObj::Bool(true);
        }

        Ok(res)
    }

    pub async fn resolve(&mut self, id_resolver: IdResolver) -> MartinResult<TileInfoSources> {
        let pg = PgBuilder::new(self, id_resolver).await?;
        let inst_tables = on_slow(
            pg.instantiate_tables(),
            // warn only if default bounds timeout has already passed
            DEFAULT_BOUNDS_TIMEOUT.add(Duration::from_secs(1)),
            || {
                if pg.auto_bounds() == BoundsCalcType::Skip {
                    warn!("Discovering tables in PostgreSQL database '{}' is taking too long. Bounds calculation is already disabled. You may need to tune your database.", pg.get_id());
                } else {
                    warn!("Discovering tables in PostgreSQL database '{}' is taking too long. Make sure your table geo columns have a GIS index, or use '--auto-bounds skip' CLI/config to skip bbox calculation.", pg.get_id());
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use indoc::indoc;
    use tilejson::Bounds;

    use super::*;
    use crate::config::tests::assert_config;
    use crate::config::Config;
    use crate::pg::config_function::FunctionInfo;
    use crate::pg::config_table::TableInfo;
    use crate::test_utils::some;
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

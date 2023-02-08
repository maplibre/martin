use futures::future::try_join;
use serde::{Deserialize, Serialize};
use tilejson::TileJSON;

use crate::config::{copy_unrecognized_config, Unrecognized};
use crate::pg::config_function::FuncInfoSources;
use crate::pg::config_table::TableInfoSources;
use crate::pg::configurator::PgBuilder;
use crate::pg::utils::Result;
use crate::source::{IdResolver, Sources};
use crate::utils::{sorted_opt_map, BoolOrObject, OneOrMany};

pub trait PgInfo {
    fn format_id(&self) -> String;
    fn to_tilejson(&self) -> TileJSON;
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PgSslCerts {
    /// Same as PGSSLCERT
    /// https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLCERT
    #[cfg(feature = "ssl")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssl_cert: Option<std::path::PathBuf>,
    /// Same as PGSSLKEY
    /// https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLKEY
    #[cfg(feature = "ssl")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssl_key: Option<std::path::PathBuf>,
    /// Same as PGSSLROOTCERT
    /// https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLROOTCERT
    #[cfg(feature = "ssl")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssl_root_cert: Option<std::path::PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PgConfig {
    pub connection_string: Option<String>,
    #[serde(flatten)]
    pub ssl_certificates: PgSslCerts,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_srid: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_bounds: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_size: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_publish: Option<BoolOrObject<PgCfgPublish>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(serialize_with = "sorted_opt_map")]
    pub tables: Option<TableInfoSources>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(serialize_with = "sorted_opt_map")]
    pub functions: Option<FuncInfoSources>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PgCfgPublish {
    pub from_schemas: Option<OneOrMany<String>>,
    pub tables: Option<BoolOrObject<PgCfgPublishType>>,
    pub functions: Option<BoolOrObject<PgCfgPublishType>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PgCfgPublishType {
    pub from_schemas: Option<OneOrMany<String>>,
    pub id_format: Option<String>,
}

impl PgConfig {
    /// Apply defaults to the config, and validate if there is a connection string
    pub fn finalize(&mut self) -> Result<Unrecognized> {
        let mut res = Unrecognized::new();
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
            self.auto_publish = Some(BoolOrObject::Bool(true));
        }

        Ok(res)
    }

    pub async fn resolve(&mut self, id_resolver: IdResolver) -> crate::Result<Sources> {
        let pg = PgBuilder::new(self, id_resolver).await?;
        let ((mut tables, tbl_info), (funcs, func_info)) =
            try_join(pg.instantiate_tables(), pg.instantiate_functions()).await?;

        self.tables = Some(tbl_info);
        self.functions = Some(func_info);
        tables.extend(funcs);
        Ok(tables)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use indoc::indoc;
    use tilejson::Bounds;

    use super::*;
    use crate::config::tests::assert_config;
    use crate::config::Config;
    use crate::pg::config_function::FunctionInfo;
    use crate::pg::config_table::TableInfo;
    use crate::test_utils::some;
    use crate::utils::OneOrMany::{Many, One};

    #[test]
    fn parse_pg_one() {
        assert_config(
            indoc! {"
            postgres:
              connection_string: 'postgresql://postgres@localhost/db'
        "},
            &Config {
                postgres: Some(One(PgConfig {
                    connection_string: some("postgresql://postgres@localhost/db"),
                    auto_publish: Some(BoolOrObject::Bool(true)),
                    ..Default::default()
                })),
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
                postgres: Some(Many(vec![
                    PgConfig {
                        connection_string: some("postgres://postgres@localhost:5432/db"),
                        auto_publish: Some(BoolOrObject::Bool(true)),
                        ..Default::default()
                    },
                    PgConfig {
                        connection_string: some("postgresql://postgres@localhost:5433/db"),
                        auto_publish: Some(BoolOrObject::Bool(true)),
                        ..Default::default()
                    },
                ])),
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
                  extent: 4096
                  buffer: 64
                  clip_geom: true
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
                postgres: Some(One(PgConfig {
                    connection_string: some("postgres://postgres@localhost:5432/db"),
                    default_srid: Some(4326),
                    pool_size: Some(20),
                    tables: Some(HashMap::from([(
                        "table_source".to_string(),
                        TableInfo {
                            schema: "public".to_string(),
                            table: "table_source".to_string(),
                            srid: 4326,
                            geometry_column: "geom".to_string(),
                            minzoom: Some(0),
                            maxzoom: Some(30),
                            bounds: Some([-180, -90, 180, 90].into()),
                            extent: Some(4096),
                            buffer: Some(64),
                            clip_geom: Some(true),
                            geometry_type: some("GEOMETRY"),
                            properties: Some(HashMap::from([(
                                "gid".to_string(),
                                "int4".to_string(),
                            )])),
                            ..Default::default()
                        },
                    )])),
                    functions: Some(HashMap::from([(
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
                })),
                ..Default::default()
            },
        );
    }
}

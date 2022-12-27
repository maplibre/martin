use futures::future::try_join;
use serde::{Deserialize, Serialize};
use tilejson::TileJSON;

use crate::config::report_unrecognized_config;
use crate::pg::config_function::FuncInfoSources;
use crate::pg::config_table::TableInfoSources;
use crate::pg::configurator::PgBuilder;
use crate::pg::pool::Pool;
use crate::pg::utils::{Result, Schemas};
use crate::source::{IdResolver, Sources};

pub trait PgInfo {
    fn format_id(&self) -> String;
    fn to_tilejson(&self) -> TileJSON;
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PgConfig {
    pub connection_string: Option<String>,
    #[cfg(feature = "ssl")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ca_root_file: Option<std::path::PathBuf>,
    #[cfg(feature = "ssl")]
    #[serde(default, skip_serializing_if = "Clone::clone")]
    pub danger_accept_invalid_certs: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_srid: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_size: Option<u32>,
    #[serde(skip)]
    pub auto_tables: Option<Schemas>,
    #[serde(skip)]
    pub auto_functions: Option<Schemas>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tables: Option<TableInfoSources>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub functions: Option<FuncInfoSources>,
    #[serde(skip)]
    pub run_autodiscovery: bool,
}

impl PgConfig {
    /// Apply defaults to the config, and validate if there is a connection string
    pub fn finalize(&mut self) -> Result<&Self> {
        if let Some(ref ts) = self.tables {
            for (k, v) in ts {
                report_unrecognized_config(&format!("tables.{k}."), &v.unrecognized);
            }
        }
        if let Some(ref fs) = self.functions {
            for (k, v) in fs {
                report_unrecognized_config(&format!("functions.{k}."), &v.unrecognized);
            }
        }
        self.run_autodiscovery = self.tables.is_none() && self.functions.is_none();

        Ok(self)
    }

    pub async fn resolve(&mut self, id_resolver: IdResolver) -> Result<(Sources, Pool)> {
        let pg = PgBuilder::new(self, id_resolver).await?;
        let ((mut tables, tbl_info), (funcs, func_info)) =
            try_join(pg.instantiate_tables(), pg.instantiate_functions()).await?;

        self.tables = Some(tbl_info);
        self.functions = Some(func_info);
        tables.extend(funcs);
        Ok((tables, pg.get_pool()))
    }

    #[must_use]
    pub fn is_autodetect(&self) -> bool {
        self.run_autodiscovery
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
            ---
            postgres:
              connection_string: 'postgresql://postgres@localhost/db'
        "},
            &Config {
                postgres: Some(One(PgConfig {
                    connection_string: some("postgresql://postgres@localhost/db"),
                    run_autodiscovery: true,
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
            ---
            postgres:
              - connection_string: 'postgres://postgres@localhost:5432/db'
              - connection_string: 'postgresql://postgres@localhost:5433/db'
        "},
            &Config {
                postgres: Some(Many(vec![
                    PgConfig {
                        connection_string: some("postgres://postgres@localhost:5432/db"),
                        run_autodiscovery: true,
                        ..Default::default()
                    },
                    PgConfig {
                        connection_string: some("postgresql://postgres@localhost:5433/db"),
                        run_autodiscovery: true,
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
            ---
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
                            properties: HashMap::from([("gid".to_string(), "int4".to_string())]),
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

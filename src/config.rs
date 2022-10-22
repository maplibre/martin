use crate::pg::config::{PgConfig, PgConfigBuilder};
use crate::prettify_error;
use crate::srv::config::{SrvConfig, SrvConfigBuilder};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io;
use std::io::prelude::*;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Config {
    #[serde(flatten)]
    pub srv: SrvConfig,
    #[serde(flatten)]
    pub pg: PgConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigBuilder {
    #[serde(flatten)]
    pub srv: SrvConfigBuilder,
    #[serde(flatten)]
    pub pg: PgConfigBuilder,
}

/// Update empty option in place with a non-empty value from the second option.
pub fn set_option<T>(first: &mut Option<T>, second: Option<T>) {
    if first.is_none() && second.is_some() {
        *first = second;
    }
}

impl ConfigBuilder {
    pub fn merge(&mut self, other: ConfigBuilder) -> &mut Self {
        self.srv.merge(other.srv);
        self.pg.merge(other.pg);
        self
    }

    /// Apply defaults to the config, and validate if there is a connection string
    pub fn finalize(self) -> io::Result<Config> {
        Ok(Config {
            srv: self.srv.finalize()?,
            pg: self.pg.finalize()?,
        })
    }
}

/// Read config from a file
pub fn read_config(file_name: &str) -> io::Result<ConfigBuilder> {
    let mut file = File::open(file_name)
        .map_err(|e| prettify_error!(e, "Unable to open config file '{}'", file_name))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| prettify_error!(e, "Unable to read config file '{}'", file_name))?;
    serde_yaml::from_str(contents.as_str())
        .map_err(|e| prettify_error!(e, "Error parsing config file '{}'", file_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pg::function_source::FunctionSource;
    use crate::pg::table_source::TableSource;
    use indoc::indoc;
    use std::collections::HashMap;
    use tilejson::Bounds;

    #[test]
    fn parse_config() {
        let yaml = indoc! {"
            ---
            connection_string: 'postgres://postgres@localhost:5432/db'
            danger_accept_invalid_certs: false
            default_srid: 4326
            keep_alive: 75
            listen_addresses: '0.0.0.0:3000'
            pool_size: 20
            worker_processes: 8

            table_sources:
              public.table_source:
                id: public.table_source
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

            function_sources:
              public.function_source:
                id: public.function_source
                schema: public
                function: function_source
                minzoom: 0
                maxzoom: 30
                bounds: [-180.0, -90.0, 180.0, 90.0]
        "};

        let config: ConfigBuilder = serde_yaml::from_str(yaml).expect("parse yaml");
        let config = config.finalize().expect("finalize");
        let expected = Config {
            srv: SrvConfig {
                keep_alive: 75,
                listen_addresses: "0.0.0.0:3000".to_string(),
                worker_processes: 8,
            },
            pg: PgConfig {
                connection_string: "postgres://postgres@localhost:5432/db".to_string(),
                ca_root_file: None,
                danger_accept_invalid_certs: false,
                default_srid: Some(4326),
                pool_size: 20,
                use_dynamic_sources: false,
                table_sources: HashMap::from([(
                    "public.table_source".to_string(),
                    Box::new(TableSource {
                        id: "public.table_source".to_string(),
                        schema: "public".to_string(),
                        table: "table_source".to_string(),
                        srid: 4326,
                        geometry_column: "geom".to_string(),
                        id_column: None,
                        minzoom: Some(0),
                        maxzoom: Some(30),
                        bounds: Some(Bounds {
                            left: -180.0,
                            bottom: -90.0,
                            right: 180.0,
                            top: 90.0,
                        }),
                        extent: Some(4096),
                        buffer: Some(64),
                        clip_geom: Some(true),
                        geometry_type: Some("GEOMETRY".to_string()),
                        properties: HashMap::from([("gid".to_string(), "int4".to_string())]),
                    }),
                )]),
                function_sources: HashMap::from([(
                    "public.function_source".to_string(),
                    Box::new(FunctionSource {
                        id: "public.function_source".to_string(),
                        schema: "public".to_string(),
                        function: "function_source".to_string(),
                        minzoom: Some(0),
                        maxzoom: Some(30),
                        bounds: Some(Bounds {
                            left: -180.0,
                            bottom: -90.0,
                            right: 180.0,
                            top: 90.0,
                        }),
                    }),
                )]),
            },
        };
        assert_eq!(config, expected);
    }
}

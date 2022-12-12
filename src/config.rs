use crate::io_error;
use crate::pg::config::PgConfig;
use crate::srv::config::{SrvConfig, SrvConfigBuilder};
use log::warn;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Config {
    #[serde(flatten)]
    pub srv: SrvConfig,
    #[serde(flatten)]
    pub pg: PgConfig,
}

#[derive(Debug, PartialEq, Deserialize)]
pub struct ConfigBuilder {
    #[serde(flatten)]
    pub srv: SrvConfigBuilder,
    #[serde(flatten)]
    pub pg: PgConfig,
    #[serde(flatten)]
    pub unrecognized: HashMap<String, Value>,
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
        self.unrecognized.extend(other.unrecognized);
        self
    }

    /// Apply defaults to the config, and validate if there is a connection string
    pub fn finalize(self) -> io::Result<Config> {
        report_unrecognized_config("", &self.unrecognized);
        Ok(Config {
            srv: self.srv.finalize()?,
            pg: self.pg.finalize()?,
        })
    }
}

pub fn report_unrecognized_config(prefix: &str, unrecognized: &HashMap<String, Value>) {
    for key in unrecognized.keys() {
        warn!("Unrecognized config key: {prefix}{key}");
    }
}

/// Read config from a file
pub fn read_config(file_name: &Path) -> io::Result<ConfigBuilder> {
    let mut file = File::open(file_name)
        .map_err(|e| io_error!(e, "Unable to open config file '{}'", file_name.display()))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| io_error!(e, "Unable to read config file '{}'", file_name.display()))?;
    serde_yaml::from_str(contents.as_str())
        .map_err(|e| io_error!(e, "Error parsing config file '{}'", file_name.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pg::config::{FunctionInfo, TableInfo};
    use crate::pg::utils::tests::some_str;
    use indoc::indoc;
    use std::collections::HashMap;
    use tilejson::Bounds;

    #[test]
    fn parse_config() {
        let yaml = indoc! {"
            ---
            connection_string: 'postgres://postgres@localhost:5432/db'
            default_srid: 4326
            keep_alive: 75
            listen_addresses: '0.0.0.0:3000'
            pool_size: 20
            worker_processes: 8

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
                connection_string: some_str("postgres://postgres@localhost:5432/db"),
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
                        geometry_type: some_str("GEOMETRY"),
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
            },
        };
        assert_eq!(config, expected);
    }
}

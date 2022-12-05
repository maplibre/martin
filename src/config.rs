use crate::io_error;
use crate::pg::config::PgConfig;
use crate::pmtiles::config::{PmtConfig, PmtConfigBuilderEnum};
use crate::srv::config::{SrvConfig, SrvConfigBuilder};
use log::warn;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::prelude::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

impl<S> OneOrMany<S> {
    fn map<T, E, F>(self, f: F) -> Result<OneOrMany<T>, E>
    where
        F: FnMut(S) -> Result<T, E>,
    {
        Ok(match self {
            Self::One(v) => OneOrMany::One(f(v)?),
            Self::Many(v) => OneOrMany::Many(v.into_iter().map(f).collect::<Result<_, _>>()?),
        })
    }

    pub fn generalize(self) -> Vec<S> {
        match self {
            Self::One(v) => vec![v],
            Self::Many(v) => v,
        }
    }

    pub fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::One(a), Self::One(b)) => Self::Many(vec![a, b]),
            (Self::One(a), Self::Many(mut b)) => {
                b.insert(0, a);
                Self::Many(b)
            }
            (Self::Many(mut a), Self::One(b)) => {
                a.push(b);
                Self::Many(a)
            }
            (Self::Many(mut a), Self::Many(b)) => {
                a.extend(b);
                Self::Many(a)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Config {
    #[serde(flatten)]
    pub srv: SrvConfig,
    #[serde(flatten)]
    pub pg: PgConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pmtiles: Option<PmtConfig>,
}

#[derive(Debug, PartialEq, Deserialize)]
pub struct ConfigBuilder {
    #[serde(flatten)]
    pub srv: SrvConfigBuilder,
    #[serde(flatten)]
    pub pg: PgConfig,
    pub pmtiles: Option<PmtConfigBuilderEnum>,
    #[serde(flatten)]
    pub unrecognized: HashMap<String, Value>,
}

/// Update empty option in place with a non-empty value from the second option.
pub fn set_option<T>(first: &mut Option<T>, second: Option<T>) {
    if first.is_none() && second.is_some() {
        *first = second;
    }
}

/// Merge two options
pub fn merge_option<T>(
    first: Option<T>,
    second: Option<T>,
    merge: impl FnOnce(T, T) -> T,
) -> Option<T> {
    match (first, second) {
        (Some(first), Some(second)) => Some(merge(first, second)),
        (None, Some(second)) => Some(second),
        (first, None) => first,
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
            pmtiles: self.pmtiles.map(|v| v.finalize()).transpose()?,
        })
    }
}

pub fn report_unrecognized_config(prefix: &str, unrecognized: &HashMap<String, Value>) {
    for key in unrecognized.keys() {
        warn!("Unrecognized config key: {prefix}{key}");
    }
}

/// Read config from a file
pub fn read_config(file_name: &str) -> io::Result<ConfigBuilder> {
    let mut file = File::open(file_name)
        .map_err(|e| io_error!(e, "Unable to open config file '{file_name}'"))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| io_error!(e, "Unable to read config file '{file_name}'"))?;
    serde_yaml::from_str(contents.as_str())
        .map_err(|e| io_error!(e, "Error parsing config file '{file_name}'"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pg::config::{FunctionInfo, TableInfo};
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
            
            pmtiles:
              paths:
                - /dir-path
                - /path/to/pmtiles2.pmtiles
              sources:
                  pm-src1: /tmp/pmtiles.pmtiles
                  pm-src2:
                    path: /tmp/pmtiles.pmtiles
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
                connection_string: Some("postgres://postgres@localhost:5432/db".to_string()),
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
                        geometry_type: Some("GEOMETRY".to_string()),
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
            // pmtiles: PmtConfig {
            //     file: Default::default(),
            // },
            pmtiles: None,
        };
        assert_eq!(config, expected);
    }
}

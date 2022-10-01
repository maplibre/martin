use crate::{pg, prettify_error, srv};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io;
use std::io::prelude::*;

#[derive(Clone, Debug, Serialize)]
pub struct Config {
    #[serde(flatten)]
    pub srv: srv::config::Config,
    #[serde(flatten)]
    pub pg: pg::config::Config,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigBuilder {
    #[serde(flatten)]
    pub srv: srv::config::ConfigBuilder,
    #[serde(flatten)]
    pub pg: pg::config::ConfigBuilder,
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

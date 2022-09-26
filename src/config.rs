use std::fs::File;
use std::io;
use std::io::prelude::*;

use serde::{Deserialize, Serialize};

use crate::function_source::FunctionSources;
use crate::table_source::TableSources;

#[derive(Clone, Debug, Serialize)]
pub struct Config {
    pub connection_string: String,
    pub ca_root_file: Option<String>,
    pub danger_accept_invalid_certs: bool,
    pub default_srid: Option<i32>,
    pub keep_alive: usize,
    pub listen_addresses: String,
    pub pool_size: u32,
    pub worker_processes: usize,
    pub table_sources: Option<TableSources>,
    pub function_sources: Option<FunctionSources>,
}

#[derive(Debug, Deserialize)]
pub struct ConfigBuilder {
    pub connection_string: Option<String>,
    pub ca_root_file: Option<String>,
    pub danger_accept_invalid_certs: Option<bool>,
    pub default_srid: Option<i32>,
    pub keep_alive: Option<usize>,
    pub listen_addresses: Option<String>,
    pub pool_size: Option<u32>,
    pub worker_processes: Option<usize>,
    pub table_sources: Option<TableSources>,
    pub function_sources: Option<FunctionSources>,
}

/// Update empty option in place with a non-empty value from the second option.
fn set_option<T>(first: &mut Option<T>, second: Option<T>) {
    if first.is_none() && second.is_some() {
        *first = second;
    }
}

impl ConfigBuilder {
    pub const KEEP_ALIVE_DEFAULT: usize = 75;
    pub const LISTEN_ADDRESSES_DEFAULT: &'static str = "0.0.0.0:3000";
    pub const POOL_SIZE_DEFAULT: u32 = 20;

    pub fn merge(&mut self, other: ConfigBuilder) -> &mut Self {
        set_option(&mut self.connection_string, other.connection_string);
        set_option(&mut self.ca_root_file, other.ca_root_file);
        set_option(
            &mut self.danger_accept_invalid_certs,
            other.danger_accept_invalid_certs,
        );
        set_option(&mut self.default_srid, other.default_srid);
        set_option(&mut self.keep_alive, other.keep_alive);
        set_option(&mut self.listen_addresses, other.listen_addresses);
        set_option(&mut self.pool_size, other.pool_size);
        set_option(&mut self.worker_processes, other.worker_processes);
        set_option(&mut self.table_sources, other.table_sources);
        set_option(&mut self.function_sources, other.function_sources);
        self
    }

    pub fn finalize(self) -> Config {
        Config {
            connection_string: self.connection_string.unwrap(),
            ca_root_file: self.ca_root_file,
            danger_accept_invalid_certs: self.danger_accept_invalid_certs.unwrap_or(false),
            default_srid: self.default_srid,
            keep_alive: self.keep_alive.unwrap_or(Self::KEEP_ALIVE_DEFAULT),
            listen_addresses: self
                .listen_addresses
                .unwrap_or_else(|| Self::LISTEN_ADDRESSES_DEFAULT.to_owned()),
            pool_size: self.pool_size.unwrap_or(Self::POOL_SIZE_DEFAULT),
            worker_processes: self.worker_processes.unwrap_or_else(num_cpus::get),
            table_sources: self.table_sources,
            function_sources: self.function_sources,
        }
    }
}

pub fn read_config(file_name: &str) -> io::Result<Config> {
    let mut file = File::open(file_name)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let config_builder: ConfigBuilder = serde_yaml::from_str(contents.as_str())
        .map_err(|e| ::std::io::Error::new(::std::io::ErrorKind::Other, e))?;

    Ok(config_builder.finalize())
}

use num_cpus;
use serde_yaml;
use std::error::Error;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::path::Path;

use super::db::PostgresPool;
use super::function_source::{get_function_sources, FunctionSources};
use super::table_source::{get_table_sources, TableSources};

#[derive(Clone, Debug, Serialize)]
pub struct Config {
  pub pool_size: u32,
  pub keep_alive: usize,
  pub worker_processes: usize,
  pub listen_addresses: String,
  pub table_sources: Option<TableSources>,
  pub function_sources: Option<FunctionSources>,
}

#[derive(Deserialize)]
struct ConfigBuilder {
  pub pool_size: Option<u32>,
  pub keep_alive: Option<usize>,
  pub worker_processes: Option<usize>,
  pub listen_addresses: Option<String>,
  pub table_sources: Option<TableSources>,
  pub function_sources: Option<FunctionSources>,
}

impl ConfigBuilder {
  pub fn finalize(self) -> Config {
    Config {
      pool_size: self.pool_size.unwrap_or(20),
      keep_alive: self.keep_alive.unwrap_or(75),
      worker_processes: self.worker_processes.unwrap_or_else(num_cpus::get),
      listen_addresses: self.listen_addresses.unwrap_or("0.0.0.0:3000".to_owned()),
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
    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.description()))?;

  Ok(config_builder.finalize())
}

fn generate_config(table_sources: TableSources, function_sources: FunctionSources) -> Config {
  let config = ConfigBuilder {
    pool_size: None,
    keep_alive: None,
    worker_processes: None,
    listen_addresses: None,
    table_sources: Some(table_sources),
    function_sources: Some(function_sources),
  };

  config.finalize()
}

pub fn build_config(config_filename: &str, pool: &PostgresPool) -> io::Result<Config> {
  if Path::new(config_filename).exists() {
    info!("Config found at {}", config_filename);
    let config = read_config(config_filename)?;
    return Ok(config);
  };

  let conn = pool
    .get()
    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.description()))?;

  let table_sources = get_table_sources(&conn)?;
  let function_sources = get_function_sources(&conn)?;

  let config = generate_config(table_sources, function_sources);

  Ok(config)
}

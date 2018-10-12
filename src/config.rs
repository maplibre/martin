use num_cpus;
use serde_yaml;
use std::error::Error;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::path::Path;

use super::cli::Args;
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
      listen_addresses: self
        .listen_addresses
        .unwrap_or_else(|| "0.0.0.0:3000".to_owned()),
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

// pub fn write_config(file_name: &str, config: Config) -> io::Result<()> {
//   let mut file = File::create(file_name)?;
//   let config = serde_yaml::to_string(&config)?;
//   file.write_all(config.as_bytes())?;
//   Ok(())
// }

pub fn build_config(pool: &PostgresPool, args: Args) -> io::Result<Config> {
  if args.flag_config.is_some() {
    let filename = args.flag_config.unwrap();
    if Path::new(&filename).exists() {
      info!("Config found at {}", filename);
      let config = read_config(&filename)?;
      return Ok(config);
    };
  };

  let conn = pool
    .get()
    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.description()))?;

  let table_sources = get_table_sources(&conn)?;
  let function_sources = get_function_sources(&conn)?;

  let config = ConfigBuilder {
    keep_alive: args.flag_keep_alive,
    listen_addresses: args.flag_listen_addresses,
    pool_size: args.flag_pool_size,
    worker_processes: args.flag_workers,
    table_sources: Some(table_sources),
    function_sources: Some(function_sources),
  };

  let config = config.finalize();
  Ok(config)
}

extern crate actix;
extern crate actix_web;
extern crate docopt;
extern crate env_logger;
extern crate futures;
extern crate tilejson;
#[macro_use]
extern crate log;
extern crate num_cpus;
extern crate postgres;
extern crate r2d2;
extern crate r2d2_postgres;
extern crate semver;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate serde_yaml;

mod app;
mod cli;
mod config;
mod coordinator_actor;
mod db;
mod db_executor;
mod function_source;
mod messages;
mod server;
mod source;
mod table_source;
mod utils;
mod worker_actor;

use docopt::Docopt;
use std::env;

use cli::{Args, USAGE};
use config::{generate_config, read_config, Config};
use db::{check_postgis_version, setup_connection_pool, PostgresPool};
use utils::prettify_error;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const REQUIRED_POSTGIS_VERSION: &str = ">= 2.4.0";

fn setup_from_config(file_name: String) -> Result<(Config, PostgresPool), std::io::Error> {
  let config = read_config(&file_name).map_err(prettify_error("Can't read config"))?;

  let pool = setup_connection_pool(&config.connection_string, Some(config.pool_size))
    .map_err(prettify_error("Can't setup connection pool"))?;

  info!("Connected to {}", config.connection_string);

  Ok((config, pool))
}

fn setup_from_database(args: Args) -> Result<(Config, PostgresPool), std::io::Error> {
  let connection_string = if args.arg_connection.is_some() {
    args.arg_connection.clone().unwrap()
  } else {
    env::var("DATABASE_URL").map_err(prettify_error("DATABASE_URL is not set"))?
  };

  let pool = setup_connection_pool(&connection_string, args.flag_pool_size)
    .map_err(prettify_error("Can't setup connection pool"))?;

  info!("Connected to {}", connection_string);

  let config = generate_config(args, connection_string, &pool)
    .map_err(prettify_error("Can't generate config"))?;

  Ok((config, pool))
}

fn start(args: Args) -> Result<actix::SystemRunner, std::io::Error> {
  info!("Starting martin v{}", VERSION);

  let config_file_name = args.flag_config.clone();
  let (config, pool) = if config_file_name.is_some() {
    let file_name = config_file_name.clone().unwrap();
    info!("Using {}", file_name);
    setup_from_config(file_name)?
  } else {
    info!("Config is not set, scanning database");
    setup_from_database(args)?
  };

  let matches = check_postgis_version(REQUIRED_POSTGIS_VERSION, &pool)
    .map_err(prettify_error("Can't check PostGIS version"))?;

  if !matches {
    std::process::exit(-1);
  }

  let watch_mode = config.watch || env::var_os("WATCH_MODE").is_some();
  if watch_mode {
    info!("Watch mode enabled");
  }

  let listen_addresses = config.listen_addresses.clone();
  let server = server::new(pool, config, watch_mode);
  info!("Martin has been started on {}.", listen_addresses);

  Ok(server)
}

fn main() {
  let env = env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "martin=info");
  env_logger::Builder::from_env(env).init();

  let args: Args = Docopt::new(USAGE)
    .and_then(|d| d.deserialize())
    .unwrap_or_else(|e| e.exit());

  if args.flag_help {
    println!("{}", USAGE);
    std::process::exit(0);
  }

  if args.flag_version {
    println!("v{}", VERSION);
    std::process::exit(0);
  }

  let server = match start(args) {
    Ok(server) => server,
    Err(error) => {
      error!("{}", error);
      std::process::exit(-1);
    }
  };

  let _ = server.run();
}

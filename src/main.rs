#[cfg_attr(feature = "cargo-clippy", allow(clippy))]
extern crate actix;
extern crate actix_web;
extern crate docopt;
extern crate env_logger;
extern crate futures;
extern crate mapbox_expressions_to_sql;
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
use semver::Version;
use semver::VersionReq;
use std::env;

use cli::{Args, USAGE};
use config::build_config;
use db::{select_postgis_verion, setup_connection_pool};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const REQUIRED_POSTGIS_VERSION: &str = ">= 2.4.0";

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

  let conn_string = args
    .arg_connection
    .clone()
    .unwrap_or_else(|| env::var("DATABASE_URL").expect("DATABASE_URL must be set"));

  info!("Starting martin v{}", VERSION);

  info!("Connecting to {}", conn_string);
  let pool = match setup_connection_pool(&conn_string, args.flag_pool_size) {
    Ok(pool) => {
      info!("Connected to postgres: {}", conn_string);
      pool
    }
    Err(error) => {
      error!("Can't connect to postgres: {}", error);
      std::process::exit(-1);
    }
  };

  match select_postgis_verion(&pool) {
    Ok(postgis_version) => {
      info!("PostGIS version: {}", postgis_version);

      let req = VersionReq::parse(REQUIRED_POSTGIS_VERSION).unwrap();
      let version = Version::parse(postgis_version.as_str()).unwrap();

      if !req.matches(&version) {
        error!("Martin requires PostGIS {}", REQUIRED_POSTGIS_VERSION);
        std::process::exit(-1);
      }
    }
    Err(error) => {
      error!("Can't get PostGIS version: {}", error);
      error!("Martin requires PostGIS {}", REQUIRED_POSTGIS_VERSION);
      std::process::exit(-1);
    }
  };

  let config = match build_config(&pool, args) {
    Ok(config) => config,
    Err(error) => {
      error!("Can't build config: {}", error);
      std::process::exit(-1);
    }
  };

  let listen_addresses = config.listen_addresses.clone();
  info!("Martin has been started on {}.", listen_addresses);

  let server = server::new(config, pool);
  let _ = server.run();
}

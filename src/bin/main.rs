#[macro_use]
extern crate log;

use docopt::Docopt;
use serde::Deserialize;
use std::error::Error;
use std::{env, io};

use martin::config::{read_config, Config, ConfigBuilder};
use martin::db::{check_postgis_version, setup_connection_pool, PostgresPool};
use martin::function_source::get_function_sources;
use martin::server;
use martin::table_source::get_table_sources;
use martin::utils::prettify_error;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const REQUIRED_POSTGIS_VERSION: &str = ">= 2.4.0";

pub const USAGE: &str = "
Martin - PostGIS Mapbox Vector Tiles server.

Usage:
  martin [options] [<connection>]
  martin -h | --help
  martin -v | --version

Options:
  -h --help               Show this screen.
  -v --version            Show version.
  --config=<path>         Path to config file.
  --keep-alive=<n>        Connection keep alive timeout [default: 75].
  --listen-addresses=<n>  The socket address to bind [default: 0.0.0.0:3000].
  --pool-size=<n>         Maximum connections pool size [default: 20].
  --watch                 Scan for new sources on sources list requests
  --workers=<n>           Number of web server workers.
";

#[derive(Debug, Deserialize)]
pub struct Args {
  pub arg_connection: Option<String>,
  pub flag_config: Option<String>,
  pub flag_help: bool,
  pub flag_keep_alive: Option<usize>,
  pub flag_listen_addresses: Option<String>,
  pub flag_pool_size: Option<u32>,
  pub flag_watch: bool,
  pub flag_version: bool,
  pub flag_workers: Option<usize>,
}

pub fn generate_config(
  args: Args,
  connection_string: String,
  pool: &PostgresPool,
) -> io::Result<Config> {
  let conn = pool
    .get()
    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.description()))?;

  let table_sources = get_table_sources(&conn)?;
  let function_sources = get_function_sources(&conn)?;

  let config = ConfigBuilder {
    connection_string,
    watch: Some(args.flag_watch),
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

extern crate actix;
extern crate actix_web;
extern crate env_logger;
extern crate futures;
extern crate postgres;
#[macro_use]
extern crate log;
extern crate mapbox_expressions_to_sql;
extern crate num_cpus;
extern crate r2d2;
extern crate r2d2_postgres;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate serde_yaml;
extern crate tilejson;

use std::env;
use std::error::Error;
use std::io;

mod config;
mod coordinator_actor;
mod db;
mod martin;
mod messages;
mod server;
mod source;
mod utils;
mod worker_actor;

static CONFIG_FILENAME: &str = "config.yaml";

fn main() {
    env_logger::init();

    let pool_size = 20; // TODO: get pool_size from config
    let conn_string: String = env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    info!("Connecting to {}", conn_string);
    let pool = match db::setup_connection_pool(&conn_string, pool_size) {
        Ok(pool) => {
            info!("Connected to postgres: {}", conn_string);
            pool
        }
        Err(error) => {
            error!("Can't connect to postgres: {}", error);
            std::process::exit(-1);
        }
    };

    let config = match pool.get()
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err.description()))
        .and_then(|conn| config::build(CONFIG_FILENAME, conn))
    {
        Ok(config) => config,
        Err(error) => {
            error!("Can't build config: {}", error);
            std::process::exit(-1);
        }
    };

    let listen_addresses = config.listen_addresses.clone();

    let server = server::new(config, pool);
    let _ = server.run();

    info!("Server has been started on {}.", listen_addresses);
}

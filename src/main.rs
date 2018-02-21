extern crate env_logger;
extern crate iron;
extern crate iron_test;
#[macro_use]
extern crate log;
extern crate logger;
extern crate lru;
extern crate mapbox_expressions_to_sql;
extern crate persistent;
extern crate r2d2;
extern crate r2d2_postgres;
extern crate regex;
extern crate rererouter;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate tilejson;
extern crate urlencoded;

use std::env;
use lru::LruCache;
use iron::prelude::Iron;

mod cache;
mod cors;
mod db;
mod martin;
mod source;
mod utils;

fn main() {
    env_logger::init();

    let conn_string: String = env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool_size = env::var("POOL_SIZE")
        .ok()
        .and_then(|pool_size| pool_size.parse::<u32>().ok())
        .unwrap_or(20);

    let cache_size = env::var("CACHE_SIZE")
        .ok()
        .and_then(|cache_size| cache_size.parse::<u32>().ok())
        .unwrap_or(16384);

    info!("Connecting to {} with pool size {}", conn_string, pool_size);
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

    let sources = match pool.get()
        .map_err(|err| err.into())
        .and_then(|conn| source::get_sources(conn))
    {
        Ok(sources) => sources,
        Err(error) => {
            error!("Can't load sources: {}", error);
            std::process::exit(-1);
        }
    };

    let tile_cache = LruCache::new(cache_size as usize);

    let chain = martin::chain(pool, sources, tile_cache);

    let port = 3000;
    let bind_addr = format!("0.0.0.0:{}", port);
    match Iron::new(chain).http(bind_addr.as_str()) {
        Ok(_) => info!("Server has been started on {}.", bind_addr),
        Err(err) => panic!("{:?}", err),
    };
}

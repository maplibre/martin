extern crate iron_test;
extern crate iron;
extern crate logger;
extern crate lru;
extern crate mapbox_expressions_to_sql;
extern crate persistent;
extern crate r2d2_postgres;
extern crate r2d2;
extern crate regex;
extern crate rererouter;
extern crate serde_json;
extern crate serde;
extern crate tilejson;
extern crate urlencoded;
#[macro_use] extern crate log;
#[macro_use] extern crate serde_derive;

use iron::prelude::Chain;
use logger::Logger;
use lru::LruCache;
use persistent::{Read, State};
use rererouter::RouterBuilder;

mod cache;
mod cors;
mod db;
mod routes;
mod tileset;

pub fn chain(conn_string: String, cache_size: usize) -> iron::Chain {
    let mut router_builder = RouterBuilder::new();
    router_builder.get(r"/index.json", routes::index);
    router_builder.get(r"/(?P<tileset>[\w|\.]*)\.json", routes::tileset);
    router_builder.get(r"/(?P<tileset>[\w|\.]*)/(?P<z>\d*)/(?P<x>\d*)/(?P<y>\d*).pbf", routes::tile);
    let router = router_builder.finalize();

    let mut chain = Chain::new(router);

    let (logger_before, logger_after) = Logger::new(None);
    chain.link_before(logger_before);

    match db::setup_connection_pool(&conn_string, 10) {
        Ok(pool) => {
            info!("Connected to postgres: {}", conn_string);
            let conn = pool.get().unwrap();
            let tilesets = tileset::get_tilesets(conn).unwrap();
            chain.link(Read::<tileset::Tilesets>::both(tilesets));

            chain.link(Read::<db::DB>::both(pool));
        },
        Err(error) => {
            error!("Can't connect to postgres: {}", error);
            std::process::exit(-1);
        }
    };

    let tile_cache = LruCache::new(cache_size);
    chain.link(State::<cache::TileCache>::both(tile_cache));

    chain.link_after(cors::Middleware);
    chain.link_after(logger_after);

    chain
}

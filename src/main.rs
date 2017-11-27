extern crate iron;
extern crate regex;
extern crate persistent;
extern crate rererouter;
extern crate r2d2;
extern crate r2d2_postgres;
// #[macro_use] extern crate lazy_static;

use std::env;
use persistent::Read;
use rererouter::RouterBuilder;
use iron::prelude::{Iron, Chain};

mod db;
mod cors;
mod routes;

fn main() {
    let conn_string: String = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");

    let mut router_builder = RouterBuilder::new();
    router_builder.get(r"/index.json", routes::index);
    router_builder.get(r"/(?P<tileset>[\w|\.]*)\.json", routes::tileset);
    router_builder.get(r"/(?P<tileset>[\w|\.]*)/(?P<z>\d*)/(?P<x>\d*)/(?P<y>\d*).pbf", routes::tile);
    let router = router_builder.finalize();

    let mut chain = Chain::new(router);

    println!("Connecting to postgres: {}", conn_string);
    match db::setup_connection_pool(&conn_string, 10) {
        Ok(pool) => {
            let conn = pool.get().unwrap();
            let tilesets = db::get_tilesets(conn).unwrap();
            chain.link(Read::<db::Tilesets>::both(tilesets));

            chain.link(Read::<db::DB>::both(pool));
        },
        Err(error) => {
            eprintln!("Error connectiong to postgres: {}", error);
            std::process::exit(-1);
        }
    };

    chain.link_after(cors::Middleware);

    let port = 3000;
    let bind_addr = format!("0.0.0.0:{}", port);
    println!("Server has been started on {}.", bind_addr);
    Iron::new(chain).http(bind_addr.as_str()).unwrap();
}

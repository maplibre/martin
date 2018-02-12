extern crate env_logger;
extern crate iron;
#[macro_use]
extern crate log;
extern crate martin_lib;

use std::env;
use iron::prelude::Iron;

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

    info!("pool_size {}, cache_size {}!", pool_size, cache_size);
    let chain = martin_lib::chain(conn_string, pool_size, cache_size);

    let port = 3000;
    let bind_addr = format!("0.0.0.0:{}", port);
    match Iron::new(chain).http(bind_addr.as_str()) {
        Ok(_) => info!("Server has been started on {}.", bind_addr),
        Err(err) => panic!("{:?}", err),
    };
}

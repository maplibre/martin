extern crate actix;
extern crate actix_web;
extern crate env_logger;
extern crate futures;
#[macro_use]
extern crate log;
extern crate r2d2;
extern crate r2d2_postgres;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;

use std::env;
use actix_web::HttpServer;
use actix::SyncArbiter;

mod db;
mod utils;
mod martin;
mod source;

fn main() {
    env_logger::init();

    let conn_string: String = env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool_size = env::var("POOL_SIZE")
        .ok()
        .and_then(|pool_size| pool_size.parse::<u32>().ok())
        .unwrap_or(20);

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

    let sys = actix::System::new("martin");
    let db_sync_arbiter = SyncArbiter::start(3, move || db::DbExecutor(pool.clone()));

    let port = 3000;
    let bind_addr = format!("0.0.0.0:{}", port);
    let _addr = HttpServer::new(move || martin::new(db_sync_arbiter.clone(), sources.clone()))
        .bind(bind_addr.clone())
        .expect(&format!("Can't bind to {}", bind_addr))
        .shutdown_timeout(0)
        .start();

    let _ = sys.run();
    info!("Server has been started on {}.", bind_addr);
}

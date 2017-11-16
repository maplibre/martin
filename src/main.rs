extern crate url;
extern crate iron;
extern crate regex;
extern crate iron_cors;
extern crate persistent;

extern crate r2d2;
extern crate r2d2_postgres;

use std::env;
use url::Url;
use regex::Regex;
use iron::prelude::*;
use iron::mime;
use iron::status;
use iron::typemap::Key;
use iron_cors::CorsMiddleware;
use persistent::Read;

use r2d2::{Pool, PooledConnection};
use r2d2_postgres::{TlsMode, PostgresConnectionManager};

pub type PostgresPool = Pool<PostgresConnectionManager>;
pub type PostgresPooledConnection = PooledConnection<PostgresConnectionManager>;

pub struct DB;
impl Key for DB { type Value = PostgresPool; }

fn setup_connection_pool(cn_str: &str, pool_size: u32) -> PostgresPool {
    let config = r2d2::Config::builder().pool_size(pool_size).build();
    let manager = PostgresConnectionManager::new(cn_str, TlsMode::None).unwrap();
    r2d2::Pool::new(config, manager).unwrap()
}

fn handler(req: &mut Request) -> IronResult<Response> {
    let url: Url = req.url.clone().into();
    let tile_re = r"^/(?P<schema>\w*)/(?P<table>\w*)/(?P<z>\d*)/(?P<x>\d*)/(?P<y>\d*).(?P<format>\w*)$";
    let re = Regex::new(tile_re).unwrap();
    match re.captures(&url.path()) {
        Some(caps) => {
            println!("{} {} {}", req.method, req.version, req.url);

            let pool = req.get::<Read<DB>>().unwrap();
            let conn = pool.get().unwrap();

            let query = format!(
                "SELECT ST_AsMVT(q, '{1}', 4096, 'geom') FROM ( \
                    SELECT ST_AsMVTGeom(                        \
                        geom,                                   \
                        TileBBox({2}, {3}, {4}, 4326),          \
                        4096,                                   \
                        256,                                    \
                        true                                    \
                    ) AS geom FROM {0}.{1}                      \
                ) AS q;",
                &caps["schema"], &caps["table"], &caps["z"], &caps["x"], &caps["y"]
            );

            match conn.query(&query, &[]) {
                Ok(rows) => {
                    let content_type = "application/x-protobuf".parse::<mime::Mime>().unwrap();
                    let tile: Vec<u8> = rows.get(0).get("st_asmvt");
                    match tile.len() {
                        0 => Ok(Response::with((content_type, status::NoContent))),
                        _ => Ok(Response::with((content_type, status::Ok, tile)))
                    }
                },
                Err(e) => Ok(Response::with((status::InternalServerError, e.to_string())))
            }
        },
        None => Ok(Response::with((status::NotFound)))
    }
}

fn main() {
    let conn_string: String = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");

    println!("connecting to postgres: {}", conn_string);
    let pool = setup_connection_pool(&conn_string, 10);

    let mut middleware = Chain::new(handler);
    middleware.link(Read::<DB>::both(pool));

    let cors_middleware = CorsMiddleware::with_allow_any(false);
    middleware.link_around(cors_middleware);

    let port = 3000;
    let bind_addr = format!("0.0.0.0:{}", port);
    println!("server has been started on {}.", bind_addr);
    Iron::new(middleware).http(bind_addr.as_str()).unwrap();
}
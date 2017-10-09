extern crate url;
extern crate iron;
extern crate regex;
extern crate persistent;
extern crate serde_json;

extern crate r2d2;
extern crate r2d2_postgres;
extern crate postgres;

use std::env;
use url::Url;
use regex::Regex;
use iron::prelude::*;
use iron::mime;
use iron::status;
use iron::typemap::Key;
use persistent::Read;
use serde_json::Value;

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

fn get_json(conn: PostgresPooledConnection, schema: &str, table: &str) -> IronResult<Response> {
    let query = format!("select json_agg({1}) from {0}.{1}", schema, table);
    match conn.query(&query, &[]) {
        Ok(rows) => {
            let content_type = "application/json".parse::<mime::Mime>().unwrap();
            let result: Value = rows.get(0).get("json_agg");
            let content = serde_json::to_string(&result).unwrap();
            Ok(Response::with((content_type, status::Ok, content)))
        },
        Err(e) => Ok(Response::with((status::InternalServerError, e.to_string())))
    }
}

fn handler(req: &mut Request) -> IronResult<Response> {
    println!("{} {} {}", req.method, req.version, req.url);

    let pool = req.get::<Read<DB>>().unwrap();
    let conn = pool.get().unwrap();

    let url: Url = req.url.clone().into();
    let re = Regex::new(r"^/(?P<schema>\w*)/(?P<table>\w*).(?P<format>\w*)$").unwrap();

    match re.captures(&url.path()) {
        Some(caps) => {
            match &caps["format"] {
                "json" => get_json(conn, &caps["schema"], &caps["table"]),
                &_ => Ok(Response::with((status::NotFound)))
            }
        },
        None => Ok(Response::with((status::NotFound)))
    }
}

fn main() {
    let conn_string: String = match env::var("DATABASE_URL") {
        Ok(val) => val,
        Err(_) => panic!("Thou shalt specify DATABASE_URL")
    };

    println!("connecting to postgres: {}", conn_string);
    let pool = setup_connection_pool(&conn_string, 10);

    let mut middleware = Chain::new(handler);
    middleware.link(Read::<DB>::both(pool));

    let port = 3000;
    let bind_addr = format!("localhost:{}", port);
    println!("server has been started on {}.", bind_addr);
    Iron::new(middleware).http(bind_addr.as_str()).unwrap();
}
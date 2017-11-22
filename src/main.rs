extern crate url;
extern crate iron;
extern crate regex;
extern crate persistent;
extern crate r2d2;
extern crate r2d2_postgres;

use std::env;
use url::Url;
use regex::Regex;
use iron::prelude::*;
use iron::headers::AccessControlAllowOrigin;
use iron::{mime, status, AfterMiddleware};
use persistent::Read;

mod db;

struct CORS;
impl AfterMiddleware for CORS {
    fn after(&self, _req: &mut Request, mut resp: Response) -> IronResult<Response> {
        resp.headers.set(AccessControlAllowOrigin::Any);
        Ok(resp)
    }
}

fn handler(req: &mut Request) -> IronResult<Response> {
    let url: Url = req.url.clone().into();
    let tile_re = r"^/(?P<schema>\w*)/(?P<table>\w*)/(?P<z>\d*)/(?P<x>\d*)/(?P<y>\d*).(?P<format>\w*)$";
    let re = Regex::new(tile_re).unwrap();
    match re.captures(&url.path()) {
        Some(caps) => {
            println!("{} {} {}", req.method, req.version, req.url);

            let pool = req.get::<Read<db::DB>>().unwrap();
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

    let mut chain = Chain::new(handler);

    println!("Connecting to postgres: {}", conn_string);
    match db::setup_connection_pool(&conn_string, 10) {
        Ok(pool) => chain.link(Read::<db::DB>::both(pool)),
        Err(error) => {
            eprintln!("Error connectiong to postgres: {}", error);
            std::process::exit(-1);
        }
    };

    chain.link_after(CORS);

    let port = 3000;
    let bind_addr = format!("0.0.0.0:{}", port);
    println!("server has been started on {}.", bind_addr);
    Iron::new(chain).http(bind_addr.as_str()).unwrap();
}
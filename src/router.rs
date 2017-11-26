use url::Url;
use regex::{Regex, Captures};
use iron::prelude::{Plugin};
use iron::{status, mime, Request, Response, IronResult};
use persistent::Read;

use super::db;

type RouteHandler = fn(&mut Request, Captures) -> IronResult<Response>;

lazy_static! {
    static ref ROUTES_VEC: Vec<(Regex, RouteHandler)> = vec![
        (Regex::new(r"^/index.json$").unwrap(), index),
        (Regex::new(r"^/(?P<tileset>[\w|\.]*)\.json$").unwrap(), tileset),
        (Regex::new(r"^/(?P<tileset>[\w|\.]*)/(?P<z>\d*)/(?P<x>\d*)/(?P<y>\d*).pbf$").unwrap(), tile)
    ];
}

pub fn handler(req: &mut Request) -> IronResult<Response> {
    println!("{} {} {}", req.method, req.version, req.url);

    let url: Url = req.url.clone().into();
    match ROUTES_VEC.clone().into_iter().find(|ref x| x.0.is_match(url.path())) {
      Some((re, handler)) => {
        let captures = re.captures(url.path()).unwrap();
        handler(req, captures)
      }
      None => Ok(Response::with((status::NotFound)))
    }
}

fn index(_req: &mut Request, _caps: Captures) -> IronResult<Response> {
  println!("index.json");
    Ok(Response::with((status::Ok, "index.json")))
}

fn tileset(_req: &mut Request, _caps: Captures) -> IronResult<Response> {
    println!("tileset {:?}", _caps);
    Ok(Response::with((status::Ok, "tileset")))
}

fn tile(req: &mut Request, caps: Captures) -> IronResult<Response> {
    let tilesets = req.get::<Read<db::Tilesets>>().unwrap();
    let tileset = match tilesets.get(&caps["tileset"]) {
        Some(tileset) => tileset,
        None => return Ok(Response::with((status::NotFound)))
    };

    let conn = match db::get_connection(req) {
        Ok(conn) => conn,
        Err(error) => {
            eprintln!("Couldn't get a connection to postgres: {}", error);
            return Ok(Response::with((status::InternalServerError)));
        }
    };

    let tile = match db::get_tile(conn, &tileset.schema, &tileset.table, &caps["z"], &caps["x"], &caps["y"]) {
        Ok(tile) => tile,
        Err(error) => {
            eprintln!("Couldn't get a tile: {}", error);
            return Ok(Response::with((status::InternalServerError)));
        }
    };

    let content_type = "application/x-protobuf".parse::<mime::Mime>().unwrap();
    match tile.len() {
        0 => Ok(Response::with((content_type, status::NoContent))),
        _ => Ok(Response::with((content_type, status::Ok, tile)))
    }
}

use regex::Captures;
use iron::prelude::{Plugin};
use iron::{status, mime, Request, Response, IronResult};
use persistent::Read;

use super::db;

pub fn index(_req: &mut Request, _caps: Captures) -> IronResult<Response> {
  println!("index.json");
    Ok(Response::with((status::Ok, "{}")))
}

pub fn tileset(_req: &mut Request, _caps: Captures) -> IronResult<Response> {
    Ok(Response::with((status::Ok, "{}")))
}

pub fn tile(req: &mut Request, caps: Captures) -> IronResult<Response> {
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

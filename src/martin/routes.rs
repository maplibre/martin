use iron::{status, mime, Request, Response, IronResult};
use iron::prelude::{Plugin};
use mapbox_expressions_to_sql;
use persistent::Read;
use regex::Captures;
use serde_json;
use urlencoded::UrlEncodedQuery;

use super::db;
use super::tileset;
use tilejson::TileJSONBuilder;

pub fn index(_req: &mut Request, _caps: Captures) -> IronResult<Response> {
    // let tilesets = req.get::<Read<db::Tilesets>>().unwrap();
    // let serialized_tilesets = serde_json::to_string(&tilesets).unwrap();
    // Ok(Response::with((status::Ok, serialized_tilesets)))

    Ok(Response::with((status::Ok, "{}")))
}

pub fn tileset(req: &mut Request, caps: Captures) -> IronResult<Response> {
    let tilesets = req.get::<Read<tileset::Tilesets>>().unwrap();
    let tileset = match tilesets.get(&caps["tileset"]) {
        Some(tileset) => tileset,
        None => return Ok(Response::with((status::NotFound)))
    };

    let mut tilejson_builder = TileJSONBuilder::new();
    tilejson_builder.name(&tileset.table);
    let tilejson = tilejson_builder.finalize();
    
    let serialized_tilejson = serde_json::to_string(&tilejson).unwrap();
    Ok(Response::with((status::Ok, serialized_tilejson)))
}

fn get_filter<'a>(req: &'a mut Request) -> Option<&'a String> {
    req.get_ref::<UrlEncodedQuery>().ok()
        .and_then(|query| query.get("filter"))
        .and_then(|filter| filter.last())
}

pub fn tile(req: &mut Request, caps: Captures) -> IronResult<Response> {
    let tilesets = req.get::<Read<tileset::Tilesets>>().unwrap();
    let tileset = match tilesets.get(&caps["tileset"]) {
        Some(tileset) => tileset,
        None => return Ok(Response::with((status::NotFound)))
    };

    let conn = match db::get_connection(req) {
        Ok(conn) => conn,
        Err(error) => {
            error!("Couldn't get a connection to postgres: {}", error);
            return Ok(Response::with((status::InternalServerError)));
        }
    };

    let z: &i32 = &caps["z"].parse().unwrap();
    let x: &i32 = &caps["x"].parse().unwrap();
    let y: &i32 = &caps["y"].parse().unwrap();

    let filter = get_filter(req).cloned();
    let condition = match filter {
        Some(filter) => {
            match mapbox_expressions_to_sql::parse(&filter) {
                Ok(condition) => Some(format!("WHERE {}", condition)),
                Err(error) => {
                    error!("Couldn't parse expression: {:?}", error);
                    return Ok(Response::with((status::InternalServerError)));
                }
            }
        },
        None => None
    };

    let tile = match tileset::get_tile(conn, &tileset, z, x, y, condition) {
        Ok(tile) => tile,
        Err(error) => {
            error!("Couldn't get a tile: {}", error);
            return Ok(Response::with((status::InternalServerError)));
        }
    };

    let content_type = "application/x-protobuf".parse::<mime::Mime>().unwrap();
    match tile.len() {
        0 => Ok(Response::with((content_type, status::NoContent))),
        _ => Ok(Response::with((content_type, status::Ok, tile)))
    }
}

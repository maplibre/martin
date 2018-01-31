use iron::{status, mime, Request, Response, IronResult};
use iron::headers::{Headers, parsing};
use iron::prelude::{Plugin};
use iron::url::Url;
use mapbox_expressions_to_sql;
use persistent::{Read, State};
use regex::{Regex, Captures};
use serde_json;
use urlencoded::UrlEncodedQuery;

use super::db;
use super::cache;
use super::tileset;
use tilejson::TileJSONBuilder;

fn get_header(headers: &Headers, name: &str, default: &str) -> String {
    headers
        .get_raw(name)
        .and_then(|h| parsing::from_one_raw_str(h).ok())
        .unwrap_or(default.to_string())
}

pub fn index(_req: &mut Request, _caps: Captures) -> IronResult<Response> {
    // let tilesets = req.get::<Read<tileset::Tilesets>>().unwrap();
    // let serialized_tilesets = serde_json::to_string(&tilesets).unwrap();
    // Ok(Response::with((status::Ok, serialized_tilesets)))

    let content_type = "application/json".parse::<mime::Mime>().unwrap();
    Ok(Response::with((content_type, status::Ok, "{}")))
}

pub fn tileset(req: &mut Request, caps: Captures) -> IronResult<Response> {
    let tilesets = req.get::<Read<tileset::Tilesets>>().unwrap();
    let tileset = match tilesets.get(&caps["tileset"]) {
        Some(tileset) => tileset,
        None => return Ok(Response::with((status::NotFound)))
    };

    let protocol = get_header(&req.headers, "x-forwarded-proto", req.url.scheme());

    let host = get_header(&req.headers, "x-forwarded-host", &req.url.host().to_string());
    let port = req.url.port();
    let host_and_port = if port == 80 || port == 443 {
        host
    } else {
        format!("{}:{}", host, port)
    };

    let original_url = get_header(&req.headers, "x-rewrite-url", &req.url.path().join("/"));
    let re = Regex::new(r"\A(.*)\.json\z").unwrap();
    let uri = match re.captures(&original_url) {
        Some(caps) => caps[1].to_string(),
        None => return Ok(Response::with((status::InternalServerError)))
    };

    let tiles_url = format!("{}://{}/{}/{{z}}/{{x}}/{{y}}.pbf", protocol, host_and_port, uri);

    let mut tilejson_builder = TileJSONBuilder::new();
    tilejson_builder.scheme("tms");
    tilejson_builder.name(&tileset.id);
    tilejson_builder.tiles(vec![&tiles_url]);

    let tilejson = tilejson_builder.finalize();
    let serialized_tilejson = serde_json::to_string(&tilejson).unwrap();

    let content_type = "application/json".parse::<mime::Mime>().unwrap();
    Ok(Response::with((content_type, status::Ok, serialized_tilejson)))
}

fn get_filter<'a>(req: &'a mut Request) -> Option<&'a String> {
    req.get_ref::<UrlEncodedQuery>().ok()
        .and_then(|query| query.get("filter"))
        .and_then(|filter| filter.last())
}

pub fn tile(req: &mut Request, caps: Captures) -> IronResult<Response> {
    let url: Url = req.url.clone().into();
    let lock = req.get::<State<cache::TileCache>>().unwrap();
    let cached_tile = lock.write().ok().and_then(|mut guard|
        guard.get(&url).cloned()
    );

    let content_type = "application/x-protobuf".parse::<mime::Mime>().unwrap();
    if let Some(tile) = cached_tile {
        debug!("{} hit", url);
        return match tile.len() {
            0 => Ok(Response::with((content_type, status::NoContent))),
            _ => Ok(Response::with((content_type, status::Ok, tile)))
        }
    };

    debug!("{} miss", url);
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

    let z: &u32 = &caps["z"].parse().unwrap();
    let x: &u32 = &caps["x"].parse().unwrap();
    let y: &u32 = &caps["y"].parse().unwrap();

    let filter = get_filter(req).cloned();
    let condition = match filter {
        Some(filter) => {
            match mapbox_expressions_to_sql::parse(&filter) {
                Ok(condition) => Some(condition),
                Err(error) => {
                    error!("Couldn't parse expression: {:?}", error);
                    return Ok(Response::with((status::InternalServerError)));
                }
            }
        },
        None => None
    };

    let query = tileset.get_query(z.clone(), x.clone(), y.clone(), condition);
    let tile: Vec<u8> = match conn.query(&query, &[]) {
        Ok(rows) => rows.get(0).get("st_asmvt"),
        Err(error) => {
            error!("Couldn't get a tile: {}", error);
            return Ok(Response::with((status::InternalServerError)));
        }
    };

    let mut guard = lock.write().unwrap();
    guard.put(req.url.clone().into(), tile.clone());

    match tile.len() {
        0 => Ok(Response::with((content_type, status::NoContent))),
        _ => Ok(Response::with((content_type, status::Ok, tile)))
    }
}

use iron::{mime, status, IronResult, Request, Response};
use iron::headers::{parsing, Headers};
use iron::prelude::{Chain, Plugin};
use iron::url::Url;
use logger::Logger;
use lru::LruCache;
use mapbox_expressions_to_sql;
use persistent::{Read, State};
use regex::{Captures, Regex};
use rererouter::RouterBuilder;
use serde_json;
use std::collections::HashMap;
use std::ops::Deref;
use urlencoded::UrlEncodedQuery;

use super::cache::TileCache;
use super::cors;
use super::db::{get_connection, PostgresPool, DB};
use super::source::{Source, Sources};
use tilejson::TileJSONBuilder;

fn get_header(headers: &Headers, name: &str, default: &str) -> String {
    headers
        .get_raw(name)
        .and_then(|h| parsing::from_one_raw_str(h).ok())
        .unwrap_or(default.to_string())
}

fn get_filter<'a>(req: &'a mut Request) -> Option<&'a String> {
    req.get_ref::<UrlEncodedQuery>()
        .ok()
        .and_then(|query| query.get("filter"))
        .and_then(|filter| filter.last())
}

pub fn index(req: &mut Request, _caps: Captures) -> IronResult<Response> {
    let sources = req.get::<Read<Sources>>().unwrap();
    let serialized_sources = serde_json::to_string(&sources.deref()).unwrap();

    let content_type = "application/json".parse::<mime::Mime>().unwrap();

    Ok(Response::with((
        content_type,
        status::Ok,
        serialized_sources,
    )))
}

pub fn source(req: &mut Request, caps: Captures) -> IronResult<Response> {
    let sources = req.get::<Read<Sources>>().unwrap();
    let source = match sources.get(&caps["source"]) {
        Some(source) => source,
        None => return Ok(Response::with(status::NotFound)),
    };

    let protocol = get_header(&req.headers, "x-forwarded-proto", req.url.scheme());

    let host = get_header(
        &req.headers,
        "x-forwarded-host",
        &req.url.host().to_string(),
    );
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
        None => return Ok(Response::with(status::InternalServerError)),
    };

    let tiles_url = format!(
        "{}://{}/{}/{{z}}/{{x}}/{{y}}.pbf",
        protocol, host_and_port, uri
    );

    let mut tilejson_builder = TileJSONBuilder::new();
    tilejson_builder.scheme("tms");
    tilejson_builder.name(&source.id);
    tilejson_builder.tiles(vec![&tiles_url]);

    let tilejson = tilejson_builder.finalize();
    let serialized_tilejson = serde_json::to_string(&tilejson).unwrap();

    let content_type = "application/json".parse::<mime::Mime>().unwrap();
    Ok(Response::with((
        content_type,
        status::Ok,
        serialized_tilejson,
    )))
}

pub fn tile(req: &mut Request, caps: Captures) -> IronResult<Response> {
    let url: Url = req.url.clone().into();
    let lock = req.get::<State<TileCache>>().unwrap();
    let cached_tile = lock.write()
        .ok()
        .and_then(|mut guard| guard.get(&url).cloned());

    let content_type = "application/x-protobuf".parse::<mime::Mime>().unwrap();
    if let Some(tile) = cached_tile {
        debug!("{} hit", url);
        return match tile.len() {
            0 => {
                debug!("{} 204", url);
                Ok(Response::with((content_type, status::NoContent)))
            }
            _ => {
                debug!("{} 200", url);
                Ok(Response::with((content_type, status::Ok, tile)))
            }
        };
    };

    debug!("{} miss", url);
    let sources = req.get::<Read<Sources>>().unwrap();
    let source = match sources.get(&caps["source"]) {
        Some(source) => source,
        None => return Ok(Response::with(status::NotFound)),
    };

    let conn = match get_connection(req) {
        Ok(conn) => conn,
        Err(error) => {
            debug!("{} 500", url);
            error!("Couldn't get a connection to postgres: {}", error);
            return Ok(Response::with(status::InternalServerError));
        }
    };

    let z: &u32 = &caps["z"].parse().unwrap();
    let x: &u32 = &caps["x"].parse().unwrap();
    let y: &u32 = &caps["y"].parse().unwrap();

    let filter = get_filter(req).cloned();
    let condition = match filter {
        Some(filter) => match mapbox_expressions_to_sql::parse(&filter) {
            Ok(condition) => Some(condition),
            Err(error) => {
                debug!("{} 500", url);
                error!("Couldn't parse expression: {:?}", error);
                return Ok(Response::with(status::InternalServerError));
            }
        },
        None => None,
    };

    let query = source.get_query(z.clone(), x.clone(), y.clone(), condition);
    let tile: Vec<u8> = match conn.query(&query, &[]) {
        Ok(rows) => rows.get(0).get("st_asmvt"),
        Err(error) => {
            debug!("{} 500", url);
            error!("Couldn't get a tile: {}", error);
            return Ok(Response::with(status::InternalServerError));
        }
    };

    let mut guard = lock.write().unwrap();
    guard.put(req.url.clone().into(), tile.clone());

    match tile.len() {
        0 => {
            debug!("{} 204", url);
            Ok(Response::with((content_type, status::NoContent)))
        }
        _ => {
            debug!("{} 200", url);
            Ok(Response::with((content_type, status::Ok, tile)))
        }
    }
}

pub fn chain(
    pool: PostgresPool,
    sources: HashMap<String, Source>,
    tile_cache: LruCache<Url, Vec<u8>>,
) -> Chain {
    let mut router_builder = RouterBuilder::new();

    router_builder.get(r"/index.json", index);
    router_builder.get(r"/(?P<source>[\w|\.]*)\.json", source);

    router_builder.get(
        r"/(?P<source>[\w|\.]*)/(?P<z>\d*)/(?P<x>\d*)/(?P<y>\d*).pbf",
        tile,
    );

    let router = router_builder.finalize();

    let mut chain = Chain::new(router);

    let (logger_before, logger_after) = Logger::new(None);
    chain.link_before(logger_before);

    chain.link(Read::<Sources>::both(sources));
    chain.link(Read::<DB>::both(pool));
    chain.link(State::<TileCache>::both(tile_cache));

    chain.link_after(cors::Middleware);
    chain.link_after(logger_after);

    chain
}

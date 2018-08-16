use actix::*;
use actix_web::*;
use futures::future::Future;
use std::collections::HashMap;

use super::config::Config;
use super::db_executor::DbExecutor;
use super::function_source::FunctionSources;
use super::messages;
use super::table_source::TableSources;
use super::utils::{build_tilejson, parse_xyz};

pub type Query = HashMap<String, String>;

pub struct State {
    db: Addr<DbExecutor>,
    table_sources: Option<TableSources>,
    function_sources: Option<FunctionSources>,
}

// TODO: Swagger endpoint
fn get_table_sources(req: &HttpRequest<State>) -> Result<HttpResponse> {
    let state = &req.state();
    let table_sources = state
        .table_sources
        .clone()
        .ok_or(error::ErrorNotFound("There is no table sources"))?;

    Ok(HttpResponse::Ok()
        .header("Access-Control-Allow-Origin", "*")
        .json(table_sources))
}

// TODO: add properties to TileJSON endpoint
fn get_table_source(req: &HttpRequest<State>) -> Result<HttpResponse> {
    let state = &req.state();
    let table_sources = state
        .table_sources
        .clone()
        .ok_or(error::ErrorNotFound("There is no table sources"))?;

    let params = req.match_info();
    let source_id = params
        .get("source_id")
        .ok_or(error::ErrorBadRequest("Invalid table source id"))?;

    let source = table_sources
        .get(source_id)
        .ok_or(error::ErrorNotFound(format!(
            "Table source '{}' not found",
            source_id
        )))?;

    let tilejson = build_tilejson(
        source.clone(),
        &req.connection_info(),
        req.path(),
        req.query_string(),
        req.headers(),
    ).map_err(|e| error::ErrorBadRequest(format!("Can't build TileJSON: {}", e)))?;

    Ok(HttpResponse::Ok()
        .header("Access-Control-Allow-Origin", "*")
        .json(tilejson))
}

fn get_table_source_tile(
    req: &HttpRequest<State>,
) -> Result<Box<Future<Item = HttpResponse, Error = Error>>> {
    let state = &req.state();
    let table_sources = state
        .table_sources
        .clone()
        .ok_or(error::ErrorNotFound("There is no table sources"))?;

    let params = req.match_info();
    let source_id = params
        .get("source_id")
        .ok_or(error::ErrorBadRequest("Invalid table source id"))?;

    let source = table_sources
        .get(source_id)
        .ok_or(error::ErrorNotFound(format!(
            "Table source '{}' not found",
            source_id
        )))?;

    let xyz = parse_xyz(params)
        .map_err(|e| error::ErrorBadRequest(format!("Can't parse XYZ scheme: {}", e)))?;

    let query = req.query();

    Ok(req
        .state()
        .db
        .send(messages::GetTile {
            xyz,
            query: query.clone(),
            source: source.clone(),
        })
        .from_err()
        .and_then(|res| match res {
            Ok(tile) => match tile.len() {
                0 => Ok(HttpResponse::NoContent()
                    .content_type("application/x-protobuf")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(tile)),
                _ => Ok(HttpResponse::Ok()
                    .content_type("application/x-protobuf")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(tile)),
            },
            Err(_) => Ok(HttpResponse::InternalServerError().into()),
        })
        .responder())
}

fn get_function_sources(req: &HttpRequest<State>) -> Result<HttpResponse> {
    let state = &req.state();
    let function_sources = state
        .function_sources
        .clone()
        .ok_or(error::ErrorNotFound("There is no function sources"))?;

    Ok(HttpResponse::Ok()
        .header("Access-Control-Allow-Origin", "*")
        .json(function_sources))
}

fn get_function_source(req: &HttpRequest<State>) -> Result<HttpResponse> {
    let state = &req.state();
    let function_sources = state
        .function_sources
        .clone()
        .ok_or(error::ErrorNotFound("There is no function sources"))?;

    let params = req.match_info();
    let source_id = params
        .get("source_id")
        .ok_or(error::ErrorBadRequest("Invalid function source id"))?;

    let source = function_sources
        .get(source_id)
        .ok_or(error::ErrorNotFound(format!(
            "Function source '{}' not found",
            source_id
        )))?;

    let tilejson = build_tilejson(
        source.clone(),
        &req.connection_info(),
        req.path(),
        req.query_string(),
        req.headers(),
    ).map_err(|e| error::ErrorBadRequest(format!("Can't build TileJSON: {}", e)))?;

    Ok(HttpResponse::Ok()
        .header("Access-Control-Allow-Origin", "*")
        .json(tilejson))
}

fn get_function_source_tile(
    req: &HttpRequest<State>,
) -> Result<Box<Future<Item = HttpResponse, Error = Error>>> {
    let state = &req.state();
    let function_sources = state
        .function_sources
        .clone()
        .ok_or(error::ErrorNotFound("There is no function sources"))?;

    let params = req.match_info();
    let source_id = params
        .get("source_id")
        .ok_or(error::ErrorBadRequest("Invalid function source id"))?;

    let source = function_sources
        .get(source_id)
        .ok_or(error::ErrorNotFound(format!(
            "Function source '{}' not found",
            source_id
        )))?;

    let xyz = parse_xyz(params)
        .map_err(|e| error::ErrorBadRequest(format!("Can't parse XYZ scheme: {}", e)))?;

    let query = req.query();

    Ok(req
        .state()
        .db
        .send(messages::GetTile {
            xyz,
            query: query.clone(),
            source: source.clone(),
        })
        .from_err()
        .and_then(|res| match res {
            Ok(tile) => match tile.len() {
                0 => Ok(HttpResponse::NoContent()
                    .content_type("application/x-protobuf")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(tile)),
                _ => Ok(HttpResponse::Ok()
                    .content_type("application/x-protobuf")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(tile)),
            },
            Err(_) => Ok(HttpResponse::InternalServerError().into()),
        })
        .responder())
}

pub fn new(db_sync_arbiter: Addr<DbExecutor>, config: Config) -> App<State> {
    let state = State {
        db: db_sync_arbiter,
        table_sources: config.table_sources,
        function_sources: config.function_sources,
    };

    App::with_state(state)
        .middleware(middleware::Logger::default())
        .resource("/index.json", |r| {
            r.method(http::Method::GET).f(get_table_sources)
        })
        .resource("/{source_id}.json", |r| {
            r.method(http::Method::GET).f(get_table_source)
        })
        .resource("/{source_id}/{z}/{x}/{y}.pbf", |r| {
            r.method(http::Method::GET).f(get_table_source_tile)
        })
        .resource("/rpc/index.json", |r| {
            r.method(http::Method::GET).f(get_function_sources)
        })
        .resource("/rpc/{source_id}.json", |r| {
            r.method(http::Method::GET).f(get_function_source)
        })
        .resource("/rpc/{source_id}/{z}/{x}/{y}.pbf", |r| {
            r.method(http::Method::GET).f(get_function_source_tile)
        })
}

use actix::*;
use actix_web::*;
use futures::future::Future;
use std::collections::HashMap;
use tilejson::TileJSONBuilder;

use super::config::Config;
use super::db_executor::DbExecutor;
// use super::function_source::FunctionSources;
use super::messages;
use super::table_source::TableSources;
use super::utils::parse_xyz;

pub type Query = HashMap<String, String>;

pub struct State {
    db: Addr<DbExecutor>,
    table_sources: Option<TableSources>,
    // function_sources: Option<FunctionSources>,
}

// TODO: Swagger endpoint
fn index(req: &HttpRequest<State>) -> Result<HttpResponse> {
    let table_sources = &req.state().table_sources.clone().unwrap();

    Ok(HttpResponse::Ok()
        .header("Access-Control-Allow-Origin", "*")
        .json(table_sources))
}

fn source(req: &HttpRequest<State>) -> Result<HttpResponse> {
    let source_id = req
        .match_info()
        .get("source_id")
        .ok_or(error::ErrorBadRequest("invalid source"))?;

    let path = req
        .headers()
        .get("x-rewrite-url")
        .map_or(String::from(source_id), |header| {
            let parts: Vec<&str> = header.to_str().unwrap().split('.').collect();
            let (_, parts_without_extension) = parts.split_last().unwrap();
            let path_without_extension = parts_without_extension.join(".");
            let (_, path_without_leading_slash) = path_without_extension.split_at(1);

            String::from(path_without_leading_slash)
        });

    let conn = req.connection_info();
    let tiles_url = format!(
        "{}://{}/{}/{{z}}/{{x}}/{{y}}.pbf",
        conn.scheme(),
        conn.host(),
        path
    );

    let mut tilejson_builder = TileJSONBuilder::new();
    tilejson_builder.scheme("tms");
    tilejson_builder.name(&source_id);
    tilejson_builder.tiles(vec![&tiles_url]);
    let tilejson = tilejson_builder.finalize();

    Ok(HttpResponse::Ok()
        .header("Access-Control-Allow-Origin", "*")
        .json(tilejson))
}

fn tile(req: &HttpRequest<State>) -> Result<Box<Future<Item = HttpResponse, Error = Error>>> {
    let sources = &req.state().table_sources.clone().unwrap();
    let params = req.match_info();
    let query = req.query();

    let source_id = params
        .get("source_id")
        .ok_or(error::ErrorBadRequest("invalid source"))?;

    let source = sources.get(source_id).ok_or(error::ErrorNotFound(format!(
        "source {} not found",
        source_id
    )))?;

    let xyz = parse_xyz(params)
        .map_err(|e| error::ErrorBadRequest(format!("Can't parse XYZ scheme: {}", e)))?;

    Ok(req
        .state()
        .db
        .send(messages::GetTile {
            xyz: xyz,
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
        // function_sources: config.function_sources,
    };

    App::with_state(state)
        .middleware(middleware::Logger::default())
        .resource("/index.json", |r| r.method(http::Method::GET).f(index))
        .resource("/{source_id}.json", |r| {
            r.method(http::Method::GET).f(source)
        })
        .resource("/{source_id}/{z}/{x}/{y}.pbf", |r| {
            r.method(http::Method::GET).f(tile)
        })
}

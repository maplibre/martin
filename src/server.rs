use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::Rc;

use actix::{Actor, Addr, SyncArbiter};
use actix_cors::Cors;
use actix_web::dev::Server;
use actix_web::http::Uri;
use actix_web::middleware::TrailingSlash;
use actix_web::web::Data;
use actix_web::{
    error, middleware, web, App, Error, HttpRequest, HttpResponse, HttpServer, Result,
};
use log::{error, info};
use serde::Deserialize;
use std::time::Duration;

use crate::composite_source::CompositeSource;
use crate::config::Config;
use crate::coordinator_actor::CoordinatorActor;
use crate::db::Pool;
use crate::db_actor::DbActor;
use crate::function_source::FunctionSources;
use crate::messages;
use crate::source::{Query, Source, Xyz};
use crate::table_source::{TableSource, TableSources};
use crate::utils::parse_x_rewrite_url;
use crate::worker_actor::WorkerActor;

pub struct AppState {
    pub db: Addr<DbActor>,
    pub coordinator: Addr<CoordinatorActor>,
    pub table_sources: Rc<RefCell<Option<TableSources>>>,
    pub function_sources: Rc<RefCell<Option<FunctionSources>>>,
    pub watch_mode: bool,
    pub default_srid: Option<i32>,
}

#[derive(Deserialize)]
struct SourceRequest {
    source_id: String,
}

#[derive(Deserialize)]
struct CompositeSourceRequest {
    source_ids: String,
}

#[derive(Deserialize)]
struct TileRequest {
    source_id: String,
    z: i32,
    x: i32,
    y: i32,
    #[allow(dead_code)]
    format: String,
}

#[derive(Deserialize)]
struct CompositeTileRequest {
    source_ids: String,
    z: i32,
    x: i32,
    y: i32,
    #[allow(dead_code)]
    format: String,
}

fn map_internal_error<T: std::fmt::Display>(e: T) -> Error {
    // FIXME: is e.to_string() needed here, or can it just be error!("{e}")  ?
    error!("{}", e.to_string());
    error::ErrorInternalServerError(e.to_string())
}

async fn get_health() -> Result<HttpResponse, Error> {
    let response = HttpResponse::Ok().body("OK");
    Ok(response)
}

async fn get_table_sources(state: web::Data<AppState>) -> Result<HttpResponse, Error> {
    if !state.watch_mode {
        let table_sources = state.table_sources.borrow().clone();
        let response = HttpResponse::Ok().json(table_sources);
        return Ok(response);
    }

    info!("Scanning database for table sources");

    let table_sources = state
        .db
        .send(messages::GetTableSources {
            default_srid: state.default_srid,
        })
        .await
        .map_err(map_internal_error)?
        .map_err(map_internal_error)?;

    state.coordinator.do_send(messages::RefreshTableSources {
        table_sources: Some(table_sources.clone()),
    });

    Ok(HttpResponse::Ok().json(table_sources))
}

async fn get_composite_source(
    req: HttpRequest,
    path: web::Path<CompositeSourceRequest>,
    state: web::Data<AppState>,
) -> Result<HttpResponse> {
    let table_sources = state
        .table_sources
        .borrow()
        .clone()
        .ok_or_else(|| error::ErrorNotFound("There is no table sources"))?;

    let sources: Vec<TableSource> = path
        .source_ids
        .split(',')
        .filter_map(|source_id| table_sources.get(source_id))
        .map(|source| source.deref().clone())
        .collect();

    if sources.is_empty() {
        return Err(error::ErrorNotFound("There is no such table sources"));
    }

    let source = CompositeSource {
        id: path.source_ids.clone(),
        table_sources: sources,
    };

    let mut tilejson = source
        .get_tilejson()
        .await
        .map_err(|e| error::ErrorBadRequest(format!("Can't build TileJSON: {e}")))?;

    let tiles_path = req
        .headers()
        .get("x-rewrite-url")
        .and_then(parse_x_rewrite_url)
        .unwrap_or_else(|| req.path().trim_end_matches(".json").to_owned());

    let connection_info = req.connection_info();

    let path_and_query = if req.query_string().is_empty() {
        format!("{tiles_path}/{{z}}/{{x}}/{{y}}.pbf")
    } else {
        format!("{tiles_path}/{{z}}/{{x}}/{{y}}.pbf?{}", req.query_string())
    };

    let tiles_url = Uri::builder()
        .scheme(connection_info.scheme())
        .authority(connection_info.host())
        .path_and_query(path_and_query)
        .build()
        .map(|tiles_url| tiles_url.to_string())
        .map_err(|e| error::ErrorBadRequest(format!("Can't build tiles URL: {e}")))?;

    tilejson.tiles = vec![tiles_url];
    Ok(HttpResponse::Ok().json(tilejson))
}

async fn get_composite_source_tile(
    path: web::Path<CompositeTileRequest>,
    state: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    let table_sources = state
        .table_sources
        .borrow()
        .clone()
        .ok_or_else(|| error::ErrorNotFound("There is no table sources"))?;

    let sources: Vec<TableSource> = path
        .source_ids
        .split(',')
        .filter_map(|source_id| table_sources.get(source_id))
        .map(|source| source.deref().clone())
        .filter(|src| is_valid_zoom(path.z, src.minzoom, src.maxzoom))
        .collect();

    if sources.is_empty() {
        return Err(error::ErrorNotFound("There is no such table sources"));
    }

    let source = CompositeSource {
        id: path.source_ids.clone(),
        table_sources: sources,
    };

    get_tile(&state.db, path.z, path.x, path.y, None, Box::new(source)).await
}

async fn get_function_sources(state: web::Data<AppState>) -> Result<HttpResponse, Error> {
    if !state.watch_mode {
        let function_sources = state.function_sources.borrow().clone();
        let response = HttpResponse::Ok().json(function_sources);
        return Ok(response);
    }

    info!("Scanning database for function sources");

    let function_sources = state
        .db
        .send(messages::GetFunctionSources {})
        .await
        .map_err(map_internal_error)?
        .map_err(map_internal_error)?;

    state.coordinator.do_send(messages::RefreshFunctionSources {
        function_sources: Some(function_sources.clone()),
    });

    Ok(HttpResponse::Ok().json(function_sources))
}

async fn get_function_source(
    req: HttpRequest,
    path: web::Path<SourceRequest>,
    state: web::Data<AppState>,
) -> Result<HttpResponse> {
    let function_sources = state
        .function_sources
        .borrow()
        .clone()
        .ok_or_else(|| error::ErrorNotFound("There is no function sources"))?;

    let source = function_sources.get(&path.source_id).ok_or_else(|| {
        error::ErrorNotFound(format!("Function source '{}' not found", path.source_id))
    })?;

    let mut tilejson = source
        .get_tilejson()
        .await
        .map_err(|e| error::ErrorBadRequest(format!("Can't build TileJSON: {e}")))?;

    let tiles_path = req
        .headers()
        .get("x-rewrite-url")
        .and_then(parse_x_rewrite_url)
        .unwrap_or_else(|| req.path().trim_end_matches(".json").to_owned());

    let connection_info = req.connection_info();

    let path_and_query = if req.query_string().is_empty() {
        format!("{tiles_path}/{{z}}/{{x}}/{{y}}.pbf")
    } else {
        format!("{tiles_path}/{{z}}/{{x}}/{{y}}.pbf?{}", req.query_string())
    };

    let tiles_url = Uri::builder()
        .scheme(connection_info.scheme())
        .authority(connection_info.host())
        .path_and_query(path_and_query)
        .build()
        .map(|tiles_url| tiles_url.to_string())
        .map_err(|e| error::ErrorBadRequest(format!("Can't build tiles URL: {e}")))?;

    tilejson.tiles = vec![tiles_url];
    Ok(HttpResponse::Ok().json(tilejson))
}

async fn get_function_source_tile(
    path: web::Path<TileRequest>,
    query: web::Query<HashMap<String, String>>,
    state: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    let function_sources = state
        .function_sources
        .borrow()
        .clone()
        .ok_or_else(|| error::ErrorNotFound("There is no function sources"))?;

    let source = function_sources
        .get(&path.source_id)
        .filter(|src| is_valid_zoom(path.z, src.minzoom, src.maxzoom))
        .ok_or_else(|| {
            error::ErrorNotFound(format!("Function source '{}' not found", path.source_id))
        })?;

    get_tile(
        &state.db,
        path.z,
        path.x,
        path.y,
        Some(query.into_inner()),
        source.clone(),
    )
    .await
}

fn is_valid_zoom(zoom: i32, minzoom: Option<u8>, maxzoom: Option<u8>) -> bool {
    let gte_minzoom = minzoom.map_or(true, |minzoom| zoom >= minzoom.into());

    let lte_maxzoom = maxzoom.map_or(true, |maxzoom| zoom <= maxzoom.into());

    gte_minzoom && lte_maxzoom
}

async fn get_tile(
    db: &Addr<DbActor>,
    z: i32,
    x: i32,
    y: i32,
    query: Option<Query>,
    source: Box<dyn Source + Send>,
) -> Result<HttpResponse, Error> {
    let message = messages::GetTile {
        xyz: Xyz { z, x, y },
        query,
        source,
    };

    let tile = db
        .send(message)
        .await
        .map_err(map_internal_error)?
        .map_err(map_internal_error)?;

    match tile.len() {
        0 => Ok(HttpResponse::NoContent()
            .content_type("application/x-protobuf")
            .body(tile)),
        _ => Ok(HttpResponse::Ok()
            .content_type("application/x-protobuf")
            .body(tile)),
    }
}

pub fn router(cfg: &mut web::ServiceConfig) {
    cfg.route("/healthz", web::get().to(get_health))
        .route("/index.json", web::get().to(get_table_sources))
        .route("/{source_ids}.json", web::get().to(get_composite_source))
        .route(
            "/{source_ids}/{z}/{x}/{y}.{format}",
            web::get().to(get_composite_source_tile),
        )
        .route("/rpc/index.json", web::get().to(get_function_sources))
        .route("/rpc/{source_id}.json", web::get().to(get_function_source))
        .route(
            "/rpc/{source_id}/{z}/{x}/{y}.{format}",
            web::get().to(get_function_source_tile),
        );
}

fn create_state(
    db: Addr<DbActor>,
    coordinator: Addr<CoordinatorActor>,
    config: Config,
) -> AppState {
    let table_sources = Rc::new(RefCell::new(config.table_sources));
    let function_sources = Rc::new(RefCell::new(config.function_sources));

    let worker_actor = WorkerActor {
        table_sources: table_sources.clone(),
        function_sources: function_sources.clone(),
    };

    let worker: Addr<_> = worker_actor.start();
    coordinator.do_send(messages::Connect { addr: worker });

    AppState {
        db,
        coordinator,
        table_sources,
        function_sources,
        watch_mode: config.watch,
        default_srid: config.default_srid,
    }
}

pub fn new(pool: Pool, config: Config) -> Server {
    let db = SyncArbiter::start(3, move || DbActor(pool.clone()));
    let coordinator: Addr<_> = CoordinatorActor::default().start();

    let keep_alive = config.keep_alive;
    let worker_processes = config.worker_processes;
    let listen_addresses = config.listen_addresses.clone();

    HttpServer::new(move || {
        let state = create_state(db.clone(), coordinator.clone(), config.clone());

        let cors_middleware = Cors::default()
            .allow_any_origin()
            .allowed_methods(vec!["GET"]);

        App::new()
            .app_data(Data::new(state))
            .wrap(cors_middleware)
            .wrap(middleware::NormalizePath::new(TrailingSlash::MergeOnly))
            .wrap(middleware::Logger::default())
            .wrap(middleware::Compress::default())
            .configure(router)
    })
    .bind(listen_addresses.clone())
    .unwrap_or_else(|_| panic!("Can't bind to {listen_addresses}"))
    .keep_alive(Duration::from_secs(keep_alive as u64))
    .shutdown_timeout(0)
    .workers(worker_processes)
    .run()
}

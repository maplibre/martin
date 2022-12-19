use crate::pg::utils::parse_x_rewrite_url;
use crate::source::{Source, UrlQuery, Xyz};
use crate::srv::config::{SrvConfig, KEEP_ALIVE_DEFAULT, LISTEN_ADDRESSES_DEFAULT};
use actix_cors::Cors;
use actix_web::dev::Server;
use actix_web::http::header::CACHE_CONTROL;
use actix_web::http::Uri;
use actix_web::middleware::TrailingSlash;
use actix_web::web::{Data, Path, Query};
use actix_web::{
    error, middleware, route, web, App, Error, HttpRequest, HttpResponse, HttpServer, Responder,
    Result,
};
use futures::future::try_join_all;
use itertools::Itertools;
use log::{debug, error};
use martin_tile_utils::DataFormat;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::time::Duration;
use tilejson::{TileJSON, VectorLayer};

/// List of keywords that cannot be used as source IDs. Some of these are reserved for future use.
/// Reserved keywords must never end in a "dot number" (e.g. ".1")
pub const RESERVED_KEYWORDS: &[&str] = &[
    "catalog", "config", "health", "help", "index", "manifest", "refresh", "reload", "status",
];

pub type Sources = HashMap<String, Box<dyn Source>>;

pub struct AppState {
    pub sources: Sources,
}

impl AppState {
    pub fn get_source(&self, id: &str) -> Result<&dyn Source> {
        Ok(self
            .sources
            .get(id)
            .ok_or_else(|| error::ErrorNotFound(format!("Source {id} does not exist")))?
            .as_ref())
    }

    fn get_sources(
        &self,
        source_ids: &str,
        zoom: Option<i32>,
    ) -> Result<(Vec<&dyn Source>, bool, DataFormat)> {
        // TODO?: optimize by pre-allocating max allowed layer count on stack
        let mut sources = Vec::new();
        let mut format: Option<DataFormat> = None;
        let mut use_url_query = false;
        for id in source_ids.split(',') {
            let src = self.get_source(id)?;
            let src_fmt = src.get_format();
            use_url_query |= src.support_url_query();

            // make sure all sources have the same format
            match format {
                Some(fmt) if fmt == src_fmt => {}
                Some(fmt) => Err(error::ErrorNotFound(format!(
                    "Cannot merge sources with {fmt:?} with {src_fmt:?}"
                )))?,
                None => format = Some(src_fmt),
            }

            // TODO: Use chained-if-let once available
            if match zoom {
                Some(zoom) if check_zoom(src, id, zoom) => true,
                None => true,
                _ => false,
            } {
                sources.push(src);
            }
        }
        let format = format.unwrap_or(DataFormat::Unknown);
        Ok((sources, use_url_query, format))
    }
}

#[derive(Deserialize)]
struct TileJsonRequest {
    source_ids: String,
}

#[derive(Deserialize)]
struct TileRequest {
    source_ids: String,
    z: i32,
    x: i32,
    y: i32,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexEntry {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_layer: Option<Vec<VectorLayer>>,
}

impl PartialOrd<Self> for IndexEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for IndexEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.id, &self.name).cmp(&(&other.id, &other.name))
    }
}

fn map_internal_error<T: std::fmt::Display>(e: T) -> Error {
    error!("{e}");
    error::ErrorInternalServerError(e.to_string())
}

/// Root path will eventually have a web front. For now, just a stub.
#[route("/", method = "GET", method = "HEAD")]
#[allow(clippy::unused_async)]
async fn get_index() -> &'static str {
    "Martin server is running. Eventually this will be a nice web front."
}

/// Return 200 OK if healthy. Used for readiness and liveness probes.
#[route("/health", method = "GET", method = "HEAD")]
#[allow(clippy::unused_async)]
async fn get_health() -> impl Responder {
    HttpResponse::Ok()
        .insert_header((CACHE_CONTROL, "no-cache"))
        .message_body("OK")
}

#[route("/catalog", method = "GET", method = "HEAD")]
#[allow(clippy::unused_async)]
async fn get_catalog(state: Data<AppState>) -> impl Responder {
    let info: Vec<_> = state
        .sources
        .iter()
        .map(|(id, src)| {
            let tilejson = src.get_tilejson();
            IndexEntry {
                id: id.clone(),
                name: tilejson.name,
                description: tilejson.description,
                attribution: tilejson.attribution,
                vector_layer: tilejson.vector_layers,
            }
        })
        .sorted()
        .collect();
    HttpResponse::Ok().json(info)
}

#[route("/{source_ids}", method = "GET", method = "HEAD")]
#[allow(clippy::unused_async)]
async fn git_source_info(
    req: HttpRequest,
    path: Path<TileJsonRequest>,
    state: Data<AppState>,
) -> Result<HttpResponse> {
    let sources = state.get_sources(&path.source_ids, None)?.0;

    let tiles_path = req
        .headers()
        .get("x-rewrite-url")
        .and_then(parse_x_rewrite_url)
        .unwrap_or_else(|| req.path().to_owned());

    let info = req.connection_info();
    let tiles_url = get_tiles_url(info.scheme(), info.host(), req.query_string(), &tiles_path)?;

    Ok(HttpResponse::Ok().json(merge_tilejson(sources, tiles_url)))
}

fn get_tiles_url(scheme: &str, host: &str, query_string: &str, tiles_path: &str) -> Result<String> {
    let path_and_query = if query_string.is_empty() {
        format!("{tiles_path}/{{z}}/{{x}}/{{y}}")
    } else {
        format!("{tiles_path}/{{z}}/{{x}}/{{y}}?{query_string}")
    };

    Uri::builder()
        .scheme(scheme)
        .authority(host)
        .path_and_query(path_and_query)
        .build()
        .map(|tiles_url| tiles_url.to_string())
        .map_err(|e| error::ErrorBadRequest(format!("Can't build tiles URL: {e}")))
}

fn merge_tilejson(sources: Vec<&dyn Source>, tiles_url: String) -> TileJSON {
    let mut tilejson = sources
        .into_iter()
        .map(Source::get_tilejson)
        .reduce(|mut accum, tj| {
            if let Some(minzoom) = tj.minzoom {
                if let Some(a) = accum.minzoom {
                    if a > minzoom {
                        accum.minzoom = tj.minzoom;
                    }
                } else {
                    accum.minzoom = tj.minzoom;
                }
            }
            if let Some(maxzoom) = tj.maxzoom {
                if let Some(a) = accum.maxzoom {
                    if a < maxzoom {
                        accum.maxzoom = tj.maxzoom;
                    }
                } else {
                    accum.maxzoom = tj.maxzoom;
                }
            }
            if let Some(bounds) = tj.bounds {
                if let Some(a) = accum.bounds {
                    accum.bounds = Some(a + bounds);
                } else {
                    accum.bounds = tj.bounds;
                }
            }

            accum
        })
        .expect("nonempty sources iter");

    tilejson.tiles.push(tiles_url);
    tilejson
}

#[route("/{source_ids}/{z}/{x}/{y}", method = "GET", method = "HEAD")]
async fn get_tile(
    req: HttpRequest,
    path: Path<TileRequest>,
    state: Data<AppState>,
) -> Result<HttpResponse> {
    let xyz = Xyz {
        z: path.z,
        x: path.x,
        y: path.y,
    };

    // Optimization for a single-source request.
    let (tile, format) = if path.source_ids.contains(',') {
        let (sources, use_url_query, format) = state.get_sources(&path.source_ids, Some(path.z))?;
        if sources.is_empty() {
            return Err(error::ErrorNotFound("No valid sources found"))?;
        }
        let query = if use_url_query {
            Some(Query::<UrlQuery>::from_query(req.query_string())?.into_inner())
        } else {
            None
        };
        let tile = try_join_all(sources.into_iter().map(|s| s.get_tile(&xyz, &query)))
            .await
            .map_err(map_internal_error)?
            .concat();
        (tile, format)
    } else {
        let id = &path.source_ids;
        let zoom = xyz.z;
        let src = state.get_source(id)?;
        if !check_zoom(src, id, zoom) {
            return Err(error::ErrorNotFound(format!(
                "Zoom {zoom} is not valid for source {id}",
            )));
        }
        let query = if src.support_url_query() {
            Some(Query::<UrlQuery>::from_query(req.query_string())?.into_inner())
        } else {
            None
        };
        let tile = src
            .get_tile(&xyz, &query)
            .await
            .map_err(map_internal_error)?;
        (tile, src.get_format())
    };

    let content = format.content_type();
    match tile.len() {
        0 => Ok(HttpResponse::NoContent().content_type(content).finish()),
        _ => Ok(HttpResponse::Ok().content_type(content).body(tile)),
    }
}

pub fn router(cfg: &mut web::ServiceConfig) {
    cfg.service(get_health)
        .service(get_index)
        .service(get_catalog)
        .service(git_source_info)
        .service(get_tile);
}

pub fn new(config: SrvConfig, sources: Sources) -> (Server, String) {
    let keep_alive = Duration::from_secs(config.keep_alive.unwrap_or(KEEP_ALIVE_DEFAULT));
    let worker_processes = config.worker_processes.unwrap_or_else(num_cpus::get);
    let listen_addresses = config
        .listen_addresses
        .unwrap_or_else(|| LISTEN_ADDRESSES_DEFAULT.to_owned());

    let server = HttpServer::new(move || {
        let state = AppState {
            sources: sources.clone(),
        };

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
    .keep_alive(keep_alive)
    .shutdown_timeout(0)
    .workers(worker_processes)
    .run();

    (server, listen_addresses)
}

pub fn check_zoom(src: &dyn Source, id: &str, zoom: i32) -> bool {
    let is_valid = src.is_valid_zoom(zoom);
    if !is_valid {
        debug!("Zoom {zoom} is not valid for source {id}");
    }
    is_valid
}

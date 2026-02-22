#[cfg(feature = "_tiles")]
use actix_web::post;
use actix_web::web::Data;
use actix_web::{HttpResponse, Responder, middleware, route};
use serde::{Deserialize, Serialize};
#[cfg(feature = "_tiles")]
use serde_json::json;
#[cfg(feature = "_tiles")]
use tracing::info;

use crate::MartinResult;
#[cfg(feature = "_catalog")]
use crate::config::file::ServerState;
#[cfg(feature = "_tiles")]
use crate::source::TileSources;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Catalog {
    #[cfg(feature = "_tiles")]
    pub tiles: martin_core::tiles::catalog::TileCatalog,
    #[cfg(feature = "sprites")]
    pub sprites: martin_core::sprites::SpriteCatalog,
    #[cfg(feature = "fonts")]
    pub fonts: martin_core::fonts::FontCatalog,
    #[cfg(feature = "styles")]
    pub styles: martin_core::styles::StyleCatalog,
}

impl Catalog {
    pub fn new(#[cfg(feature = "_catalog")] state: &ServerState) -> MartinResult<Self> {
        Ok(Self {
            #[cfg(feature = "_tiles")]
            tiles: state.tiles.get_catalog(),
            #[cfg(feature = "sprites")]
            sprites: state.sprites.get_catalog()?,
            #[cfg(feature = "fonts")]
            fonts: state.fonts.get_catalog(),
            #[cfg(feature = "styles")]
            styles: state.styles.get_catalog(),
        })
    }
}

#[route(
    "/catalog",
    method = "GET",
    method = "HEAD",
    wrap = "middleware::Compress::default()"
)]
async fn get_catalog(catalog: Data<Catalog>) -> impl Responder {
    HttpResponse::Ok().json(catalog)
}

/// Refresh endpoint to re-discover tile sources.
///
/// Re-runs table/function discovery and updates the catalog
/// without requiring a server restart.
///
/// # Current Implementation
/// This endpoint provides the infrastructure for dynamic catalog updates.
/// Currently returns the current catalog state. Full `PostgreSQL` re-discovery
/// requires passing the database configuration and pool to this endpoint,
/// which will be implemented in a follow-up PR.
///
/// The `TileSources` struct now includes `upsert_sources()` and `remove_sources()`
/// methods that enable thread-safe catalog updates using the underlying `DashMap`.
///
/// # Security
/// Should be restricted to localhost or internal networks only in production.
#[cfg(feature = "_tiles")]
#[post("/_/refresh", wrap = "middleware::Compress::default()")]
async fn post_refresh(tiles: Data<TileSources>) -> impl Responder {
    info!("Refresh endpoint called");

    // TODO: Re-run PostgreSQL discovery when config/pool are available
    // For now, return current state to prove endpoint works
    // Example future implementation:
    // let builder = PostgresAutoDiscoveryBuilder::new(&config, id_resolver).await?;
    // let (new_sources, _, _) = builder.instantiate_tables().await?;
    // let (added, updated) = tiles.upsert_sources(new_sources);

    let source_count = tiles.source_names().len();
    let sources = tiles.source_names();

    info!("Refresh complete - {source_count} sources in catalog");

    HttpResponse::Ok().json(json!({
        "status": "ok",
        "message": "Refresh endpoint is operational. Full PostgreSQL re-discovery will be added in a follow-up PR.",
        "source_count": source_count,
        "sources": sources,
    }))
}

#[cfg(all(feature = "webui", not(docsrs)))]
pub mod webui {
    include!(concat!(env!("OUT_DIR"), "/generated.rs"));
}

/// Root path in case web front is disabled.
#[cfg(any(not(feature = "webui"), docsrs))]
#[route("/", method = "GET", method = "HEAD")]
async fn get_index_no_ui() -> &'static str {
    "Martin server is running. The WebUI feature was disabled at the compile time.\n\n\
    A list of all available sources is available at http://<host>/catalog\n\n\
    See documentation https://github.com/maplibre/martin"
}

/// Root path in case web front is disabled and the `webui` feature is enabled.
#[cfg(all(feature = "webui", not(docsrs)))]
#[route("/", method = "GET", method = "HEAD")]
async fn get_index_ui_disabled() -> &'static str {
    "Martin server is running.\n\n
    The WebUI feature can be enabled with the --webui enable-for-all CLI flag or in the config file, making it available to all users.\n\n
    A list of all available sources is available at http://<host>/catalog\n\n\
    See documentation https://github.com/maplibre/martin"
}

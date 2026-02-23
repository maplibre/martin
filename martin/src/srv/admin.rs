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
#[cfg(feature = "postgres")]
use crate::config::file::postgres::{PostgresAutoDiscoveryBuilder, PostgresConfig};
#[cfg(feature = "postgres")]
use crate::config::primitives::{IdResolver, OptOneMany};
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
async fn get_catalog(
    catalog: Data<Catalog>,
    #[cfg(feature = "_tiles")]
    tiles: Data<TileSources>,
) -> impl Responder {
    // Build a fresh catalog from current sources to reflect any refresh updates
    #[cfg(feature = "_tiles")]
    let fresh_catalog = Catalog {
        tiles: tiles.get_catalog(),
        #[cfg(feature = "sprites")]
        sprites: catalog.sprites.clone(),
        #[cfg(feature = "fonts")]
        fonts: catalog.fonts.clone(),
        #[cfg(feature = "styles")]
        styles: catalog.styles.clone(),
    };
    
    #[cfg(not(feature = "_tiles"))]
    let fresh_catalog = catalog.as_ref();
    
    HttpResponse::Ok().json(fresh_catalog)
}

/// Reload endpoint to re-discover tile sources.
///
/// Re-runs table/function discovery and updates the catalog
/// without requiring a server restart.
///
/// # Implementation
/// This endpoint re-discovers `PostgreSQL` tables and functions by:
/// 1. Re-running the discovery process for each configured `PostgreSQL` connection
/// 2. Updating the `Arc<DashMap>`-based catalog with newly discovered sources
/// 3. Returning statistics about added and updated sources
///
/// The catalog is updated atomically using `DashMap`, ensuring thread-safety
/// without explicit locking.
///
/// # Security
/// Should be restricted to localhost or internal networks only in production.
#[cfg(all(feature = "_tiles", feature = "postgres"))]
#[post("/_/reload", wrap = "middleware::Compress::default()")]
async fn post_reload(
    tiles: Data<TileSources>,
    postgres_configs: Data<OptOneMany<PostgresConfig>>,
) -> actix_web::Result<impl Responder> {
    info!("Reload endpoint called - re-discovering PostgreSQL sources");

    let mut total_added = 0;
    let mut total_updated = 0;
    let id_resolver = IdResolver::default();

    // Re-discover sources for each PostgreSQL configuration
    for config in postgres_configs.iter() {
        match PostgresAutoDiscoveryBuilder::new(config, id_resolver.clone()).await {
            Ok(builder) => {
                match builder.instantiate_tables().await {
                    Ok((new_sources, _, warnings)) => {
                        // Log any warnings
                        for warning in warnings {
                            tracing::warn!("Discovery warning: {warning}");
                        }

                        // Update the catalog
                        let (added, updated) = tiles.upsert_sources(new_sources);
                        total_added += added;
                        total_updated += updated;

                        let current_count = tiles.source_names().len();
                        info!("Discovered {added} new sources, updated {updated} existing sources, total now: {current_count}");
                    }
                    Err(e) => {
                        tracing::error!("Failed to instantiate tables: {e}");
                        return Ok(HttpResponse::InternalServerError().json(json!({
                            "status": "error",
                            "message": format!("Failed to discover tables: {e}"),
                        })));
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to create PostgreSQL builder: {e}");
                return Ok(HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": format!("Failed to connect to PostgreSQL: {e}"),
                })));
            }
        }
    }

    let source_count = tiles.source_names().len();
    let sources = tiles.source_names();

        info!(
            "Reload complete - {total_added} added, {total_updated} updated, {source_count} total sources"
        );

    Ok(HttpResponse::Ok().json(json!({
        "status": "success",
        "added": total_added,
        "updated": total_updated,
        "total_sources": source_count,
        "sources": sources,
    })))
}

// Stub endpoint when postgres feature is not enabled
#[cfg(all(feature = "_tiles", not(feature = "postgres")))]
#[post("/_/reload", wrap = "middleware::Compress::default()")]
async fn post_reload(tiles: Data<TileSources>) -> impl Responder {
    info!("Reload endpoint called (PostgreSQL feature not enabled)");

    let source_count = tiles.source_names().len();
    let sources = tiles.source_names();

    HttpResponse::Ok().json(json!({
        "status": "ok",
        "message": "Reload endpoint requires PostgreSQL feature to be enabled",
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

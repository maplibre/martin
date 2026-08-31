use actix_web::web::{Data, Path};
use actix_web::{HttpResponse, Responder, route};

use crate::tile_source_manager::TileSourceManager;

/// Drops every cached tile of one source.
///
/// The body says whether tiles were dropped or no tile cache is configured, and an unknown source answers `404`.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        delete,
        path = "/cache/{source_id}",
        params(("source_id" = String, Path, description = "Source ID")),
        responses(
            (status = 200, description = "The source's cached tiles are gone, or there was no tile cache to drop them from. The body says which.", body = String),
            (status = 404, description = "No such source"),
        ),
    )
)]
#[route("/cache/{source_id}", method = "DELETE")]
pub async fn purge_source(
    source_id: Path<String>,
    tile_manager: Data<TileSourceManager>,
) -> actix_web::Result<impl Responder> {
    tile_manager.tile_sources().get_source(&source_id)?;
    let Some(cache) = tile_manager.tile_cache() else {
        return Ok(HttpResponse::Ok().body(format!(
            "Source {source_id} has no cached tiles, tile caching is disabled"
        )));
    };
    cache.invalidate_source(&source_id);
    cache.run_pending_tasks().await;
    Ok(HttpResponse::Ok().body(format!("Purged the cached tiles of source {source_id}")))
}

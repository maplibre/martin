use actix_web::web::{Data, Path};
use actix_web::{HttpResponse, Responder, route};

use crate::tile_source_manager::TileSourceManager;

/// Drops every cached tile of one source.
///
/// Answers `404` for an unknown source and `204` otherwise, also when no tile cache is configured.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        delete,
        path = "/cache/{source_id}",
        params(("source_id" = String, Path, description = "Source ID")),
        responses(
            (status = 204, description = "The source's cached tiles are gone"),
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
    if let Some(cache) = tile_manager.tile_cache() {
        cache.invalidate_source(&source_id);
        cache.run_pending_tasks().await;
    }
    Ok(HttpResponse::NoContent())
}

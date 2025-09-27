//! OGC API tile serving endpoint

use actix_web::web::{Data, Path};
use actix_web::{HttpMessage, HttpRequest, HttpResponse, Result as ActixResult, route};
use serde::Deserialize;

use crate::source::TileSources;
use crate::srv::tiles::DynTileSource;
use martin_core::cache::OptMainCache;

#[derive(Deserialize)]
pub struct OgcTilePath {
    collection_id: String,
    tilematrixset_id: String,
    tile_matrix: u8,
    tile_row: u32,
    tile_col: u32,
}

/// OGC API Tile endpoint - get actual tile data
#[route(
    "/api/collections/{collection_id}/tiles/{tilematrixset_id}/{tile_matrix}/{tile_row}/{tile_col}",
    method = "GET",
    method = "HEAD"
)]
pub async fn get_ogc_tile(
    req: HttpRequest,
    path: Path<OgcTilePath>,
    sources: Data<TileSources>,
    cache: Data<OptMainCache>,
    srv_config: Data<crate::config::file::srv::SrvConfig>,
) -> ActixResult<HttpResponse> {
    // For now, we only support WebMercatorQuad
    if path.tilematrixset_id != "WebMercatorQuad" {
        return Err(actix_web::error::ErrorNotFound("TileMatrixSet not found"));
    }

    // OGC uses row/column notation, but internally Martin uses x/y
    // In Web Mercator, column = x and row = y (with y inverted)
    let y = (1 << path.tile_matrix) - 1 - path.tile_row;

    // Create tile source
    let src = DynTileSource::new(
        sources.as_ref(),
        &path.collection_id,
        Some(path.tile_matrix),
        req.query_string(),
        req.get_header::<actix_web::http::header::AcceptEncoding>(),
        req.get_header::<actix_web::http::header::IfNoneMatch>(),
        srv_config.preferred_encoding,
        cache.as_ref().as_ref(),
    )?;

    // Get tile using Martin internal coordinate system
    src.get_http_response(martin_tile_utils::TileCoord {
        z: path.tile_matrix,
        x: path.tile_col,
        y,
    })
    .await
}

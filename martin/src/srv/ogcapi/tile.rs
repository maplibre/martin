use actix_web::http::header::{CONTENT_TYPE, HeaderValue};
use actix_web::web::{Data, Path};
use actix_web::{HttpRequest, HttpResponse, Result as ActixResult, route};
use martin_tile_utils::{Format, TileCoord};
use serde::Deserialize;

use super::tilesets::tileset_source;
use crate::config::file::srv::SrvConfig;
use crate::srv::tiles::content::serve_tile;
use crate::tile_source_manager::TileSourceManager;

/// The media type OGC API - Tiles names for tiles of `format`.
pub(super) const fn media_type(format: Format) -> &'static str {
    match format {
        Format::Mvt => "application/vnd.mapbox-vector-tile",
        Format::Gif
        | Format::Jpeg
        | Format::Json
        | Format::Mlt
        | Format::Png
        | Format::Webp
        | Format::Avif
        | Format::Jxl => format.content_type(),
    }
}

/// The path of a tile, its row and column in the OGC order.
#[derive(Deserialize, Debug)]
pub struct TilePath {
    collection_id: String,
    tile_matrix_set_id: String,
    tile_matrix: u8,
    tile_row: u32,
    tile_col: u32,
}

/// One tile of a collection, answered as the `/{source_ids}/{z}/{x}/{y}` route answers it.
#[route(
    "/ogc/collections/{collection_id}/tiles/{tile_matrix_set_id}/{tile_matrix}/{tile_row}/{tile_col}",
    method = "GET",
    method = "HEAD"
)]
pub async fn get_tile(
    req: HttpRequest,
    path: Path<TilePath>,
    srv_config: Data<SrvConfig>,
    manager: Data<TileSourceManager>,
) -> ActixResult<HttpResponse> {
    manager.wait_for_sources(&path.collection_id).await;
    tileset_source(&manager, &path.collection_id, &path.tile_matrix_set_id)?;
    let xyz = TileCoord::new_unchecked(path.tile_matrix, path.tile_col, path.tile_row);
    let mut response = serve_tile(&req, &srv_config, &manager, &path.collection_id, xyz).await?;
    if response
        .headers()
        .get(CONTENT_TYPE)
        .is_some_and(|value| value == Format::Mvt.content_type())
    {
        response
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static(media_type(Format::Mvt)));
    }
    Ok(response)
}

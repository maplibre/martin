use actix_web::http::header::{CONTENT_TYPE, HeaderValue};
use actix_web::web::{Data, Path};
use actix_web::{HttpRequest, HttpResponse, Result as ActixResult, route};
use martin_tile_utils::{Format, TileCoord};
use serde::Deserialize;

use super::tilesets::tileset_source;
use crate::config::file::srv::SrvConfig;
use crate::srv::tiles::content::serve_tile;
use crate::tile_source_manager::TileSourceManager;

/// The path of a tile, its row and column in the OGC order.
#[derive(Deserialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "unstable-schemas", into_params(parameter_in = Path))]
pub struct TilePath {
    collection_id: String,
    tile_matrix_set_id: String,
    tile_matrix: u8,
    tile_row: u32,
    tile_col: u32,
}

/// One tile of a collection, answered as the `/{source_ids}/{z}/{x}/{y}` route answers it.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/collections/{collection_id}/tiles/{tile_matrix_set_id}/{tile_matrix}/{tile_row}/{tile_col}",
        operation_id = "get_ogc_tile",
        params(TilePath),
        responses(
            (status = 200, description = "Encoded vector or raster tile"),
            (status = 204, description = "Source returned an empty tile"),
            (status = 304, description = "ETag matched If-None-Match"),
            (status = 404, description = "No such collection, tileset or tile"),
            (status = 406, description = "No supported tile format in Accept header"),
        ),
    )
)]
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
            .insert(CONTENT_TYPE, HeaderValue::from_static(Format::Mvt.ogc_content_type()));
    }
    Ok(response)
}

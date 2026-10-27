use std::num::NonZeroU8;

use actix_middleware_etag::Etag;
use actix_web::http::header::{HeaderValue, VARY};
use actix_web::middleware::Compress;
use actix_web::web::{Data, Path};
use actix_web::{HttpRequest, HttpResponse, Result as ActixResult, route};
use martin_core::styles::{RenderStyle, StyleError, StyleSources, TileSize};
use martin_tile_utils::{MAX_ZOOM, WEB_MERCATOR_QUAD, WEB_MERCATOR_QUAD_ID};
use serde::Deserialize;
use tilejson::TileJSON;

use super::collections::covering_bounds;
use super::data_type::DataType;
use super::error::OgcApiError;
use super::generated_style::{Drawn, generated_style};
use super::link::path_segment;
use super::map::{MapFormat, Viewport, WEB_MERCATOR, dataset_collections, own_tiles_url, render_failure, renders_maps};
use super::tilesets::{Tiled, tile_matrix_set_limits};
use crate::config::file::srv::SrvConfig;
use crate::srv::server::map_error;
use crate::tile_source_manager::TileSourceManager;

/// Side of a map tile in pixels, as `WebMercatorQuad` defines it.
const MAP_TILE_PIXELS: u32 = 256;

/// The path of a map tile of the dataset.
#[derive(Deserialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "unstable-schemas", into_params(parameter_in = Path))]
pub struct DatasetMapTilePath {
    tile_matrix_set_id: String,
    tile_matrix: u8,
    tile_row: u32,
    tile_col: u32,
}

/// The path of a map tile of a style.
#[derive(Deserialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "unstable-schemas", into_params(parameter_in = Path))]
pub struct StyleMapTilePath {
    style_id: String,
    tile_matrix_set_id: String,
    tile_matrix: u8,
    tile_row: u32,
    tile_col: u32,
}

/// The map tilesets under `tiles_path`, of the one tileset on `WebMercatorQuad`.
fn map_tiled(tiles_path: String, title: String) -> Tiled<'static> {
    Tiled {
        tiles_path,
        title,
        data_type: DataType::Raster,
        crs: WEB_MERCATOR.to_owned(),
        grid: &WEB_MERCATOR_QUAD,
    }
}

/// Fails unless `tile_matrix_set` is `WebMercatorQuad`, the only grid maps are tiled on.
fn web_mercator_only(tile_matrix_set: &str) -> ActixResult<()> {
    if tile_matrix_set == WEB_MERCATOR_QUAD_ID {
        Ok(())
    } else {
        Err(map_error(&OgcApiError::UnknownMapTileset {
            tile_matrix_set: tile_matrix_set.to_owned(),
        }))
    }
}

/// Tile `z`/`row`/`col` of `style` in the format the `Accept` header of `req` asks for.
async fn render_map_tile(
    req: &HttpRequest,
    styles: &StyleSources,
    style: RenderStyle,
    z: u8,
    row: u32,
    col: u32,
) -> ActixResult<HttpResponse> {
    if !WEB_MERCATOR_QUAD.is_valid(z, col, row) {
        return Err(map_error(&OgcApiError::MapTileOutsideGrid { z, row, col }));
    }
    let format = MapFormat::of(req)?;
    let pixels = TileSize::Px256;
    match styles.render_of_size(style, z, col, row, pixels, NonZeroU8::MIN).await {
        Ok(image) => {
            let mut response = format
                .response(
                    image,
                    Viewport::tile(&WEB_MERCATOR_QUAD, z, col, row, MAP_TILE_PIXELS),
                    styles.image_quality(),
                )
                .await;
            response.headers_mut().append(VARY, HeaderValue::from_static("accept"));
            Ok(response)
        }
        Err(e) => Ok(render_failure(&e)),
    }
}

/// The style drawing every collection that has a map, and the bounds covering them.
fn dataset_style(req: &HttpRequest, srv_config: &SrvConfig, manager: &TileSourceManager) -> (RenderStyle, [f64; 4]) {
    let collections = dataset_collections(manager, None).unwrap_or_default();
    let described: Vec<(&str, TileJSON)> = collections
        .iter()
        .map(|(id, (source, process))| (id.as_str(), process.advertised_tilejson(source.get_tilejson().clone())))
        .collect();
    let drawn: Vec<_> = described.iter().map(|(id, tilejson)| Drawn { id, tilejson }).collect();
    let style = generated_style(&own_tiles_url(req, srv_config), &drawn);
    let bounds = covering_bounds(described.iter().map(|(_, tilejson)| tilejson.bounds));
    (RenderStyle::Json(style.to_string().into()), bounds)
}

/// The map tilesets of all collections drawn together.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/map/tiles",
        operation_id = "ogc.dataset.map.getTileSetsList",
        responses(
            (status = 200, description = "Map tilesets of all collections drawn together", body = super::tilesets::TileSets),
        ),
    )
)]
#[route(
    "/ogc/map/tiles",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
pub async fn get_dataset_map_tilesets(req: HttpRequest, srv_config: Data<SrvConfig>) -> HttpResponse {
    if !renders_maps(&req) {
        return render_failure(&StyleError::RenderingIsDisabled);
    }
    let tiled = map_tiled("/map/tiles".to_owned(), "Map of all collections".to_owned());
    HttpResponse::Ok().json(tiled.list(&req, &srv_config))
}

/// The map tileset of all collections drawn together, on `WebMercatorQuad`.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/map/tiles/{tile_matrix_set_id}",
        operation_id = "ogc.dataset.map.getTileSet",
        params(("tile_matrix_set_id" = String, Path, description = "Tile matrix set ID, only `WebMercatorQuad`")),
        responses(
            (status = 200, description = "Map tileset metadata with the tile URL template", body = super::tilesets::TileSet),
            (status = 404, description = "No such tileset"),
        ),
    )
)]
#[route(
    "/ogc/map/tiles/{tile_matrix_set_id}",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
pub async fn get_dataset_map_tileset(
    req: HttpRequest,
    path: Path<String>,
    srv_config: Data<SrvConfig>,
    manager: Data<TileSourceManager>,
) -> ActixResult<HttpResponse> {
    if !renders_maps(&req) {
        return Ok(render_failure(&StyleError::RenderingIsDisabled));
    }
    web_mercator_only(&path)?;
    manager.wait_until_loaded().await;
    let (_, [left, bottom, right, top]) = dataset_style(&req, &srv_config, &manager);
    let limits = tile_matrix_set_limits(
        &WEB_MERCATOR_QUAD,
        Some(tilejson::Bounds::new(left, bottom, right, top)),
        0..=MAX_ZOOM,
    );
    let tiled = map_tiled("/map/tiles".to_owned(), "Map of all collections".to_owned());
    Ok(HttpResponse::Ok().json(tiled.tileset(&req, &srv_config, limits, Vec::new(), "image/png")))
}

/// A map tile of all collections drawn together.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/map/tiles/{tile_matrix_set_id}/{tile_matrix}/{tile_row}/{tile_col}",
        operation_id = "ogc.dataset.map.getTile",
        params(DatasetMapTilePath),
        responses(
            (status = 200, description = "Map tile of all collections drawn together", content(("image/png"), ("image/jpeg"), ("image/tiff; application=geotiff"))),
            (status = 404, description = "No such tileset or tile"),
            (status = 406, description = "No supported image format in Accept header"),
        ),
    )
)]
#[route(
    "/ogc/map/tiles/{tile_matrix_set_id}/{tile_matrix}/{tile_row}/{tile_col}",
    method = "GET",
    method = "HEAD"
)]
pub async fn get_dataset_map_tile(
    req: HttpRequest,
    path: Path<DatasetMapTilePath>,
    srv_config: Data<SrvConfig>,
    manager: Data<TileSourceManager>,
    styles: Data<StyleSources>,
) -> ActixResult<HttpResponse> {
    web_mercator_only(&path.tile_matrix_set_id)?;
    manager.wait_until_loaded().await;
    let (style, _) = dataset_style(&req, &srv_config, &manager);
    render_map_tile(&req, &styles, style, path.tile_matrix, path.tile_row, path.tile_col).await
}

/// The map tilesets of a style.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/styles/{style_id}/map/tiles",
        operation_id = "ogc.dataset.style.map.getTileSetsList",
        params(("style_id" = String, Path, description = "Style ID")),
        responses(
            (status = 200, description = "Map tilesets of the style", body = super::tilesets::TileSets),
            (status = 404, description = "No such style"),
        ),
    )
)]
#[route(
    "/ogc/styles/{style_id}/map/tiles",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
pub async fn get_style_map_tilesets(
    req: HttpRequest,
    path: Path<String>,
    srv_config: Data<SrvConfig>,
    styles: Data<StyleSources>,
) -> ActixResult<HttpResponse> {
    if !renders_maps(&req) {
        return Ok(render_failure(&StyleError::RenderingIsDisabled));
    }
    let style_id = path.into_inner();
    if styles.style_json_path(&style_id).is_none() {
        return Err(map_error(&OgcApiError::UnknownStyle { style: style_id }));
    }
    let tiled = map_tiled(format!("/styles/{}/map/tiles", path_segment(&style_id)), style_id);
    Ok(HttpResponse::Ok().json(tiled.list(&req, &srv_config)))
}

/// The map tileset of a style on `WebMercatorQuad`.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/styles/{style_id}/map/tiles/{tile_matrix_set_id}",
        operation_id = "ogc.dataset.style.map.getTileSet",
        params(("style_id" = String, Path, description = "Style ID"), ("tile_matrix_set_id" = String, Path, description = "Tile matrix set ID, only `WebMercatorQuad`")),
        responses(
            (status = 200, description = "Map tileset metadata with the tile URL template", body = super::tilesets::TileSet),
            (status = 404, description = "No such style or tileset"),
        ),
    )
)]
#[route(
    "/ogc/styles/{style_id}/map/tiles/{tile_matrix_set_id}",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
pub async fn get_style_map_tileset(
    req: HttpRequest,
    path: Path<(String, String)>,
    srv_config: Data<SrvConfig>,
    styles: Data<StyleSources>,
) -> ActixResult<HttpResponse> {
    if !renders_maps(&req) {
        return Ok(render_failure(&StyleError::RenderingIsDisabled));
    }
    let (style_id, tile_matrix_set) = path.into_inner();
    web_mercator_only(&tile_matrix_set)?;
    if styles.style_json_path(&style_id).is_none() {
        return Err(map_error(&OgcApiError::UnknownStyle { style: style_id }));
    }
    let limits = tile_matrix_set_limits(&WEB_MERCATOR_QUAD, None, 0..=MAX_ZOOM);
    let tiled = map_tiled(format!("/styles/{}/map/tiles", path_segment(&style_id)), style_id);
    Ok(HttpResponse::Ok().json(tiled.tileset(&req, &srv_config, limits, Vec::new(), "image/png")))
}

/// A map tile of a style.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/styles/{style_id}/map/tiles/{tile_matrix_set_id}/{tile_matrix}/{tile_row}/{tile_col}",
        operation_id = "ogc.dataset.style.map.getTile",
        params(StyleMapTilePath),
        responses(
            (status = 200, description = "Map tile of the style", content(("image/png"), ("image/jpeg"), ("image/tiff; application=geotiff"))),
            (status = 404, description = "No such style, tileset or tile"),
            (status = 406, description = "No supported image format in Accept header"),
        ),
    )
)]
#[route(
    "/ogc/styles/{style_id}/map/tiles/{tile_matrix_set_id}/{tile_matrix}/{tile_row}/{tile_col}",
    method = "GET",
    method = "HEAD"
)]
pub async fn get_style_map_tile(
    req: HttpRequest,
    path: Path<StyleMapTilePath>,
    styles: Data<StyleSources>,
) -> ActixResult<HttpResponse> {
    web_mercator_only(&path.tile_matrix_set_id)?;
    let Some(style_path) = styles.style_json_path(&path.style_id) else {
        return Err(map_error(&OgcApiError::UnknownStyle {
            style: path.style_id.clone(),
        }));
    };
    render_map_tile(
        &req,
        &styles,
        style_path.into(),
        path.tile_matrix,
        path.tile_row,
        path.tile_col,
    )
    .await
}

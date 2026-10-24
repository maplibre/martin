use actix_middleware_etag::Etag;
use actix_web::middleware::Compress;
use actix_web::web::{Data, Path};
use actix_web::{HttpRequest, HttpResponse, Result as ActixResult, route};
use martin_tile_utils::{MAX_ZOOM, TileGrid};
use serde::{Deserialize, Serialize};
use tilejson::TileJSON;

use super::collections::collection_source;
use super::data_type::DataType;
use super::error::OgcApiError;
use super::link::{Link, ogc_url, path_segment};
use super::tile_matrix_sets::registered_uri;
use crate::config::file::srv::SrvConfig;
use crate::source::SourceEntry;
use crate::srv::server::map_error;
use crate::tile_source_manager::TileSourceManager;

/// The tilesets of a collection.
#[derive(Serialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
struct TileSets {
    tilesets: Vec<TileSetItem>,
    links: Vec<Link>,
}

/// A tileset in a list, linking to its metadata.
#[derive(Serialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
struct TileSetItem {
    title: String,
    data_type: DataType,
    crs: String,
    #[serde(rename = "tileMatrixSetURI", skip_serializing_if = "Option::is_none")]
    tile_matrix_set_uri: Option<String>,
    links: Vec<Link>,
}

/// The metadata of a tileset.
#[derive(Serialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
struct TileSet {
    title: String,
    data_type: DataType,
    crs: String,
    #[serde(rename = "tileMatrixSetURI", skip_serializing_if = "Option::is_none")]
    tile_matrix_set_uri: Option<String>,
    tile_matrix_set_limits: Vec<TileMatrixLimits>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    layers: Vec<Layer>,
    links: Vec<Link>,
}

/// The tiles of one zoom that hold data.
#[derive(Serialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
struct TileMatrixLimits {
    tile_matrix: String,
    min_tile_row: u32,
    max_tile_row: u32,
    min_tile_col: u32,
    max_tile_col: u32,
}

/// A layer of a vector tileset.
#[derive(Serialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
struct Layer {
    id: String,
    data_type: DataType,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_tile_matrix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tile_matrix: Option<String>,
}

/// The path of a tileset.
#[derive(Deserialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "unstable-schemas", into_params(parameter_in = Path))]
pub struct TileSetPath {
    collection_id: String,
    tile_matrix_set_id: String,
}

/// The source behind `collection` and the OGC URI of its CRS, when it is served on `tile_matrix_set`.
pub(super) fn tileset_source(
    manager: &TileSourceManager,
    collection: &str,
    tile_matrix_set: &str,
) -> ActixResult<(SourceEntry, String)> {
    let (entry, crs) = collection_source(manager, collection)?;
    if entry.0.tile_grid().id() != tile_matrix_set {
        return Err(map_error(&OgcApiError::UnknownTileset {
            collection: collection.to_owned(),
            tile_matrix_set: tile_matrix_set.to_owned(),
        }));
    }
    Ok((entry, crs))
}

/// The tiles of each zoom of the source that its bounds cover, every column where they cross the antimeridian, or the whole grid where they can't be placed.
fn tile_matrix_set_limits(grid: &TileGrid, tilejson: &TileJSON) -> Vec<TileMatrixLimits> {
    let mut bounds = tilejson
        .bounds
        .and_then(|bounds| grid.crs_bounds([bounds.left, bounds.bottom, bounds.right, bounds.top]))
        .unwrap_or_else(|| grid.bounds());
    if bounds[0] > bounds[2] {
        let whole = grid.bounds();
        (bounds[0], bounds[2]) = (whole[0], whole[2]);
    }
    let maxzoom = tilejson.maxzoom.unwrap_or(MAX_ZOOM).min(MAX_ZOOM);
    (tilejson.minzoom.unwrap_or(0)..=maxzoom)
        .map(|zoom| {
            let rect = grid.tile_range(bounds, zoom);
            TileMatrixLimits {
                tile_matrix: zoom.to_string(),
                min_tile_row: rect.min_y,
                max_tile_row: rect.max_y,
                min_tile_col: rect.min_x,
                max_tile_col: rect.max_x,
            }
        })
        .collect()
}

/// The tilesets of one collection, one per tile matrix set it is served on.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/collections/{collection_id}/tiles",
        params(("collection_id" = String, Path, description = "Collection ID, the ID of a source")),
        responses(
            (status = 200, description = "Tilesets of the collection", body = TileSets),
            (status = 404, description = "No such collection"),
        ),
    )
)]
#[route(
    "/ogc/collections/{collection_id}/tiles",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
pub async fn get_tilesets(
    req: HttpRequest,
    path: Path<String>,
    srv_config: Data<SrvConfig>,
    manager: Data<TileSourceManager>,
) -> ActixResult<HttpResponse> {
    let id = path.into_inner();
    manager.wait_for_sources(&id).await;
    let ((source, process), crs) = collection_source(&manager, &id)?;
    let grid = source.tile_grid();
    let tms = path_segment(grid.id());
    let segment = path_segment(&id);
    let tilejson = process.advertised_tilejson(source.get_tilejson().clone());
    let tileset = TileSetItem {
        title: tilejson.name.unwrap_or_else(|| id.clone()),
        data_type: DataType::of(process.advertised_tile_info(source.get_tile_info()).format),
        crs,
        tile_matrix_set_uri: registered_uri(grid),
        links: vec![
            Link::json(
                ogc_url(&req, &srv_config, &format!("/collections/{segment}/tiles/{tms}")),
                "self",
                "Tileset metadata",
            ),
            Link::json(
                ogc_url(&req, &srv_config, &format!("/tileMatrixSets/{tms}")),
                "http://www.opengis.net/def/rel/ogc/1.0/tiling-scheme",
                "Tile matrix set definition",
            ),
        ],
    };
    Ok(HttpResponse::Ok().json(TileSets {
        tilesets: vec![tileset],
        links: vec![Link::json(
            ogc_url(&req, &srv_config, &format!("/collections/{segment}/tiles")),
            "self",
            "This document",
        )],
    }))
}

/// The metadata of one tileset, with the template of its tile URLs.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/collections/{collection_id}/tiles/{tile_matrix_set_id}",
        params(TileSetPath),
        responses(
            (status = 200, description = "Tileset metadata with the tile URL template", body = TileSet),
            (status = 404, description = "No such collection or tileset"),
        ),
    )
)]
#[route(
    "/ogc/collections/{collection_id}/tiles/{tile_matrix_set_id}",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
pub async fn get_tileset(
    req: HttpRequest,
    path: Path<TileSetPath>,
    srv_config: Data<SrvConfig>,
    manager: Data<TileSourceManager>,
) -> ActixResult<HttpResponse> {
    let TileSetPath {
        collection_id: id,
        tile_matrix_set_id: tms,
    } = path.into_inner();
    manager.wait_for_sources(&id).await;
    let ((source, process), crs) = tileset_source(&manager, &id, &tms)?;
    let tms = path_segment(&tms);
    let segment = path_segment(&id);
    let grid = source.tile_grid();
    let tilejson = process.advertised_tilejson(source.get_tilejson().clone());
    let format = process.advertised_tile_info(source.get_tile_info()).format;
    let data_type = DataType::of(format);
    let layers = match data_type {
        DataType::Vector => tilejson
            .vector_layers
            .iter()
            .flatten()
            .map(|layer| Layer {
                id: layer.id.clone(),
                data_type,
                min_tile_matrix: layer.minzoom.map(|zoom| zoom.to_string()),
                max_tile_matrix: layer.maxzoom.map(|zoom| zoom.to_string()),
            })
            .collect(),
        DataType::Raster => Vec::new(),
    };
    Ok(HttpResponse::Ok().json(TileSet {
        tile_matrix_set_limits: tile_matrix_set_limits(grid, &tilejson),
        title: tilejson.name.unwrap_or_else(|| id.clone()),
        data_type,
        crs,
        tile_matrix_set_uri: registered_uri(grid),
        layers,
        links: vec![
            Link::json(
                ogc_url(&req, &srv_config, &format!("/collections/{segment}/tiles/{tms}")),
                "self",
                "This document",
            ),
            Link::json(
                ogc_url(&req, &srv_config, &format!("/tileMatrixSets/{tms}")),
                "http://www.opengis.net/def/rel/ogc/1.0/tiling-scheme",
                "Tile matrix set definition",
            ),
            Link {
                href: ogc_url(
                    &req,
                    &srv_config,
                    &format!("/collections/{segment}/tiles/{tms}/{{tileMatrix}}/{{tileRow}}/{{tileCol}}"),
                ),
                rel: "item",
                media_type: format.ogc_content_type(),
                title: "Tiles",
                templated: true,
            },
        ],
    }))
}

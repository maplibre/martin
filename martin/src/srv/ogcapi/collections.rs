use actix_middleware_etag::Etag;
use actix_web::middleware::Compress;
use actix_web::web::{Data, Path};
use actix_web::{HttpRequest, HttpResponse, Result as ActixResult, route};
use serde::Serialize;

use super::data_type::DataType;
use super::error::OgcApiError;
use super::link::{Link, ogc_url};
use super::tile_matrix_sets::{CRS84, crs};
use crate::config::file::srv::SrvConfig;
use crate::source::SourceEntry;
use crate::srv::server::map_error;
use crate::tile_source_manager::TileSourceManager;

/// The collections the OGC API serves.
#[derive(Serialize, Debug)]
struct Collections {
    collections: Vec<Collection>,
    links: Vec<Link>,
}

/// A tile source as an OGC API collection.
#[derive(Serialize, Debug)]
struct Collection {
    id: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    extent: Extent,
    links: Vec<Link>,
}

/// Where a collection's data lies.
#[derive(Serialize, Debug)]
struct Extent {
    spatial: SpatialExtent,
}

/// The bounding box of a collection, in longitude and latitude.
#[derive(Serialize, Debug)]
struct SpatialExtent {
    bbox: [[f64; 4]; 1],
    crs: &'static str,
}

/// The source behind collection `id` and the OGC URI of its CRS.
pub(super) fn collection_source(manager: &TileSourceManager, id: &str) -> ActixResult<(SourceEntry, String)> {
    manager
        .tile_sources()
        .get_source(id)
        .ok()
        .and_then(|entry| {
            let (crs, _) = crs(entry.0.tile_grid())?;
            Some((entry, crs))
        })
        .ok_or_else(|| map_error(&OgcApiError::UnknownCollection(id.to_owned())))
}

/// Collection `id`, linking to itself and to its tilesets.
fn collection(req: &HttpRequest, srv_config: &SrvConfig, id: &str, (source, process): &SourceEntry) -> Collection {
    let tilejson = process.advertised_tilejson(source.get_tilejson().clone());
    let data_type = DataType::of(process.advertised_tile_info(source.get_tile_info()).format);
    let bounds = tilejson.bounds.unwrap_or_default();
    Collection {
        id: id.to_owned(),
        title: tilejson.name.unwrap_or_else(|| id.to_owned()),
        description: tilejson.description,
        extent: Extent {
            spatial: SpatialExtent {
                bbox: [[bounds.left, bounds.bottom, bounds.right, bounds.top]],
                crs: CRS84,
            },
        },
        links: vec![
            Link::json(
                ogc_url(req, srv_config, &format!("/collections/{id}")),
                "self",
                "This collection",
            ),
            Link::json(
                ogc_url(req, srv_config, &format!("/collections/{id}/tiles")),
                data_type.tilesets_rel(),
                "Tilesets",
            ),
        ],
    }
}

/// Every tile source on a tile matrix set with an OGC definition, as a collection.
#[route(
    "/ogc/collections",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
pub async fn get_collections(
    req: HttpRequest,
    srv_config: Data<SrvConfig>,
    manager: Data<TileSourceManager>,
) -> HttpResponse {
    manager.wait_until_loaded().await;
    let sources = manager.tile_sources();
    let mut ids = sources.source_names();
    ids.sort();
    let collections = ids
        .iter()
        .filter_map(|id| {
            let entry = sources.get_source(id).ok()?;
            crs(entry.0.tile_grid())?;
            Some(collection(&req, &srv_config, id, &entry))
        })
        .collect();
    HttpResponse::Ok().json(Collections {
        collections,
        links: vec![Link::json(
            ogc_url(&req, &srv_config, "/collections"),
            "self",
            "This document",
        )],
    })
}

/// One collection.
#[route(
    "/ogc/collections/{collection_id}",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
pub async fn get_collection(
    req: HttpRequest,
    path: Path<String>,
    srv_config: Data<SrvConfig>,
    manager: Data<TileSourceManager>,
) -> ActixResult<HttpResponse> {
    let id = path.into_inner();
    manager.wait_for_sources(&id).await;
    let (entry, _) = collection_source(&manager, &id)?;
    Ok(HttpResponse::Ok().json(collection(&req, &srv_config, &id, &entry)))
}

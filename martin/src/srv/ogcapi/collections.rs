use actix_middleware_etag::Etag;
use actix_web::middleware::Compress;
use actix_web::web::{Data, Path};
use actix_web::{HttpRequest, HttpResponse, Result as ActixResult, route};
use serde::Serialize;

use super::data_type::DataType;
use super::error::OgcApiError;
use super::link::{Link, ogc_url, path_segment};
use super::tile_matrix_sets::{CRS84, crs};
use crate::config::file::srv::SrvConfig;
use crate::source::SourceEntry;
use crate::srv::server::map_error;
use crate::tile_source_manager::TileSourceManager;

/// The collections the OGC API serves.
#[derive(Serialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
struct Collections {
    collections: Vec<Collection>,
    links: Vec<Link>,
}

/// A tile source as an OGC API collection.
#[derive(Serialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
struct Collection {
    id: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    extent: Extent,
    /// The CRSs the collection is served in, only the CRS of its grid.
    crs: [String; 1],
    /// The CRS of the collection's grid.
    storage_crs: String,
    links: Vec<Link>,
}

/// Where a collection's data lies.
#[derive(Serialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
struct Extent {
    spatial: SpatialExtent,
}

/// The bounding box of a collection, in longitude and latitude and in the CRS of its grid.
#[derive(Serialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
struct SpatialExtent {
    bbox: [[f64; 4]; 1],
    crs: &'static str,
    /// `bbox` in the CRS of the collection's grid, in its axis order.
    #[serde(skip_serializing_if = "Option::is_none")]
    storage_crs_bbox: Option<[[f64; 4]; 1]>,
}

/// The one source collection `id` resolves to, as tile requests resolve it, and the OGC URI of its CRS.
pub(super) fn collection_source(manager: &TileSourceManager, id: &str) -> ActixResult<(SourceEntry, String)> {
    manager
        .tile_sources()
        .get_sources(id, None)
        .ok()
        .and_then(|resolved| {
            let mut sources = resolved.sources;
            let entry = sources.pop().filter(|_| sources.is_empty())?;
            let crs = crs(entry.0.tile_grid())?.uri;
            Some((entry, crs))
        })
        .ok_or_else(|| map_error(&OgcApiError::UnknownCollection(id.to_owned())))
}

/// Collection `id` in `storage_crs`, linking to itself, its tilesets and its map.
fn collection(
    req: &HttpRequest,
    srv_config: &SrvConfig,
    id: &str,
    entry: &SourceEntry,
    storage_crs: String,
) -> Collection {
    let (source, process) = entry;
    let tilejson = process.advertised_tilejson(source.get_tilejson().clone());
    let data_type = DataType::of(process.advertised_tile_info(source.get_tile_info()).format);
    let bounds = tilejson.bounds.unwrap_or_default();
    let bbox = [bounds.left, bounds.bottom, bounds.right, bounds.top];
    let grid = source.tile_grid();
    let storage_bbox = grid.crs_bounds(bbox).unwrap_or_else(|| grid.bounds());
    let storage_crs_bbox = crs(grid).map(|storage| [storage.bbox(storage_bbox)]);
    let segment = path_segment(id);
    #[cfg_attr(
        not(all(feature = "rendering", target_os = "linux")),
        expect(unused_mut, reason = "only maps add links")
    )]
    let mut links = vec![
        Link::json(
            ogc_url(req, srv_config, &format!("/collections/{segment}")),
            "self",
            "This collection",
        ),
        Link::json(
            ogc_url(req, srv_config, &format!("/collections/{segment}/tiles")),
            data_type.tilesets_rel(),
            "Tilesets",
        ),
    ];
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    if super::map::renders_maps(req) && super::map::can_draw(entry) {
        links.push(Link {
            href: ogc_url(req, srv_config, &format!("/collections/{segment}/map")),
            rel: super::map::MAP_REL,
            media_type: "image/png",
            title: "Map of the collection",
            templated: false,
        });
    }
    Collection {
        id: id.to_owned(),
        title: tilejson.name.unwrap_or_else(|| id.to_owned()),
        description: tilejson.description,
        extent: Extent {
            spatial: SpatialExtent {
                bbox: [bbox],
                crs: CRS84,
                storage_crs_bbox,
            },
        },
        crs: [storage_crs.clone()],
        storage_crs,
        links,
    }
}

/// Every tile source on a tile matrix set with an OGC definition, as a collection.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/collections",
        operation_id = "getCollectionsList",
        responses(
            (status = 200, description = "Every tile source on a tile matrix set with an OGC definition", body = Collections),
        ),
    )
)]
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
    let mut ids = manager.tile_sources().source_names();
    ids.sort();
    let collections = ids
        .iter()
        .filter_map(|id| {
            let (entry, storage_crs) = collection_source(&manager, id).ok()?;
            Some(collection(&req, &srv_config, id, &entry, storage_crs))
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
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/collections/{collection_id}",
        operation_id = "getCollection",
        params(("collection_id" = String, Path, description = "Collection ID, the ID of a source")),
        responses(
            (status = 200, description = "Collection", body = Collection),
            (status = 404, description = "No such collection"),
        ),
    )
)]
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
    let (entry, storage_crs) = collection_source(&manager, &id)?;
    Ok(HttpResponse::Ok().json(collection(&req, &srv_config, &id, &entry, storage_crs)))
}

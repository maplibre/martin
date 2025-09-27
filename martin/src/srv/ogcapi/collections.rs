//! OGC API collections endpoints

use actix_web::http::header::CONTENT_TYPE;
use actix_web::web::{Data, Path};
use actix_web::{HttpRequest, HttpResponse, Result as ActixResult, route};
use log::warn;
use martin_core::tiles::Source;
use ogcapi_types::common::{
    Bbox, Collection as OgcCollection, Collections as OgcCollections, Crs, Extent as OgcExtent,
    Link, SpatialExtent as OgcSpatialExtent,
};
use ogcapi_types::tiles::{
    BoundingBox2D, DataType, GeospatialData, TileMatrixLimits, TilePoint, TileSet, TileSetItem,
    TileSets, TitleDescriptionKeywords,
};
use serde::Deserialize;
use serde_json::Map;

use super::landing::get_base_url;
use crate::source::TileSources;
use crate::srv::server::Catalog;

#[derive(Deserialize)]
pub struct CollectionPath {
    collection_id: String,
}

#[derive(Deserialize)]
pub struct CollectionTileMatrixSetPath {
    collection_id: String,
    tilematrixset_id: String,
}

/// Convert tilejson bounds to OGC API spatial extent
fn bounds_to_spatial_extent(bounds: &tilejson::Bounds) -> OgcSpatialExtent {
    // Create a Bbox2D with the bounds in [west, south, east, north] order
    let bbox = Bbox::Bbox2D([bounds.left, bounds.bottom, bounds.right, bounds.top]);

    // Construct the spatial extent directly
    OgcSpatialExtent {
        bbox: vec![bbox],
        crs: Crs::new(ogcapi_types::common::Authority::OGC, "1.3", "CRS84"),
    }
}

/// Create an OGC API Collection from a tile source
fn create_collection(
    id: String,
    source_result: Result<Box<dyn Source>, actix_web::Error>,
    base_url: &str,
) -> Option<OgcCollection> {
    match source_result {
        Ok(source) => {
            let tj = source.get_tilejson();

            // Build the collection using ogcapi_types::common::Collection
            let collection = OgcCollection {
                id: id.clone(),
                title: tj.name.clone(),
                description: tj.description.clone(),
                keywords: vec![],
                attribution: tj.attribution.clone(),
                extent: tj.bounds.map(|bounds| OgcExtent {
                    spatial: Some(bounds_to_spatial_extent(&bounds)),
                    temporal: None,
                }),
                item_type: Some("tile".to_string()),
                crs: vec![Crs::from_epsg(3857)],
                storage_crs: Some(Crs::from_epsg(3857)),
                storage_crs_coordinate_epoch: None,
                links: vec![
                    Link::new(format!("{base_url}/api/collections/{id}"), "self")
                        .mediatype("application/json")
                        .title(format!("Collection {id}")),
                    Link::new(
                        format!("{base_url}/api/collections/{id}/tiles"),
                        "http://www.opengis.net/def/rel/ogc/1.0/tilesets-vector",
                    )
                    .mediatype("application/json")
                    .title(format!("Vector tilesets for collection {id}")),
                ],
                additional_properties: {
                    let mut props = Map::new();
                    props.insert("dataType".to_string(), serde_json::json!(DataType::Vector));
                    props
                },
            };

            Some(collection)
        }
        Err(e) => {
            warn!("Failed to get source for collection '{id}': {e}");
            None
        }
    }
}

/// Create OGC API Collections response from catalog and sources
fn create_collections(_catalog: &Catalog, sources: &TileSources, base_url: &str) -> OgcCollections {
    let mut collection = OgcCollections::new(
        sources
            .source_names()
            .into_iter()
            .filter_map(|id| {
                let source_result = sources.get_source(&id);
                create_collection(id, source_result, base_url)
            })
            .collect(),
    );
    collection.links = vec![
        Link::new(format!("{base_url}/api/collections"), "self")
            .mediatype("application/json")
            .title("All Collections"),
    ];
    collection
}

/// OGC API Collections endpoint
#[route("/api/collections", method = "GET", method = "HEAD")]
pub async fn get_collections(
    req: HttpRequest,
    _catalog: Data<Catalog>,
    sources: Data<TileSources>,
) -> ActixResult<HttpResponse> {
    let base_url = get_base_url(&req);
    let collections = create_collections(&_catalog, &sources, &base_url);

    Ok(HttpResponse::Ok()
        .insert_header((CONTENT_TYPE, "application/json"))
        .json(collections))
}

/// OGC API Collection endpoint
#[route("/api/collections/{collection_id}", method = "GET", method = "HEAD")]
pub async fn get_collection(
    req: HttpRequest,
    path: Path<CollectionPath>,
    _catalog: Data<Catalog>,
    sources: Data<TileSources>,
) -> ActixResult<HttpResponse> {
    let base_url = get_base_url(&req);

    // Get the specific collection
    let collection = create_collection(
        path.collection_id.clone(),
        sources.get_source(&path.collection_id),
        &base_url,
    )
    .ok_or_else(|| actix_web::error::ErrorNotFound("Collection not found"))?;

    Ok(HttpResponse::Ok()
        .insert_header((CONTENT_TYPE, "application/json"))
        .json(collection))
}

/// OGC API Collection Tiles endpoint (list of tilesets for a collection)
#[route(
    "/api/collections/{collection_id}/tiles",
    method = "GET",
    method = "HEAD"
)]
pub async fn get_collection_tiles(
    req: HttpRequest,
    path: Path<CollectionPath>,
    sources: Data<TileSources>,
) -> ActixResult<HttpResponse> {
    let base_url = get_base_url(&req);

    // Check if collection exists
    let source = sources
        .get_source(&path.collection_id)
        .map_err(|_| actix_web::error::ErrorNotFound("Collection not found"))?;

    let tj = source.get_tilejson();

    // Create a tileset item for this collection
    let tileset = TileSetItem {
        title: tj.name.clone(),
        data_type: DataType::Vector,
        crs: Crs::from_epsg(3857),
        tile_matrix_set_uri: Some(
            "http://www.opengis.net/def/tilematrixset/OGC/1.0/WebMercatorQuad".to_string(),
        ),
        links: vec![
            Link {
                rel: "self".to_string(),
                r#type: Some("application/json".to_string()),
                title: Some(format!("Tileset for {}", path.collection_id)),
                href: format!(
                    "{}/api/collections/{}/tiles/WebMercatorQuad",
                    base_url, path.collection_id
                ),
                hreflang: None,
                length: None,
            },
            Link {
                rel: "http://www.opengis.net/def/rel/ogc/1.0/tiling-scheme".to_string(),
                r#type: Some("application/json".to_string()),
                title: Some("TileMatrixSet".to_string()),
                href: format!("{}/api/tileMatrixSets/WebMercatorQuad", base_url),
                hreflang: None,
                length: None,
            },
            Link {
                rel: "item".to_string(),
                r#type: Some("application/vnd.mapbox-vector-tile".to_string()),
                title: Some("Tiles".to_string()),
                href: format!(
                    "{}/api/collections/{}/tiles/WebMercatorQuad/{{tileMatrix}}/{{tileRow}}/{{tileCol}}",
                    base_url, path.collection_id
                ),
                hreflang: None,
                length: None,
            },
        ],
    };

    let response = TileSets {
        tilesets: vec![tileset],
        links: Some(vec![Link {
            rel: "self".to_string(),
            r#type: Some("application/json".to_string()),
            title: Some("This document".to_string()),
            href: format!("{}/api/collections/{}/tiles", base_url, path.collection_id),
            hreflang: None,
            length: None,
        }]),
    };

    Ok(HttpResponse::Ok()
        .insert_header((CONTENT_TYPE, "application/json"))
        .json(response))
}

/// OGC API Collection Tileset endpoint
#[route(
    "/api/collections/{collection_id}/tiles/{tilematrixset_id}",
    method = "GET",
    method = "HEAD"
)]
pub async fn get_collection_tileset(
    req: HttpRequest,
    path: Path<CollectionTileMatrixSetPath>,
    sources: Data<TileSources>,
) -> ActixResult<HttpResponse> {
    let base_url = get_base_url(&req);

    // For now, we only support WebMercatorQuad
    if path.tilematrixset_id != "WebMercatorQuad" {
        return Err(actix_web::error::ErrorNotFound("TileMatrixSet not found"));
    }

    // Check if collection exists
    let source = sources
        .get_source(&path.collection_id)
        .map_err(|_| actix_web::error::ErrorNotFound("Collection not found"))?;

    let tj = source.get_tilejson();

    let tileset = TileSet {
        title_description_keywords: TitleDescriptionKeywords {
            title: tj.name.clone(),
            description: tj.description.clone(),
            keywords: None,
        },
        data_type: DataType::Vector,
        tile_matrix_set_uri: Some(
            "http://www.opengis.net/def/tilematrixset/OGC/1.0/WebMercatorQuad".to_string(),
        ),
        tile_matrix_set_limits: tj.minzoom.and_then(|min| {
            tj.maxzoom.map(|max| {
                (min..=max)
                    .map(|z| TileMatrixLimits {
                        tile_matrix: z.to_string(),
                        min_tile_row: 0,
                        max_tile_row: ((1 << z) - 1) as u64,
                        min_tile_col: 0,
                        max_tile_col: ((1 << z) - 1) as u64,
                    })
                    .collect()
            })
        }),
        crs: Crs::from_epsg(3857),
        epoch: None,
        links: vec![
            Link {
                rel: "self".to_string(),
                r#type: Some("application/json".to_string()),
                title: Some("This document".to_string()),
                href: format!(
                    "{}/api/collections/{}/tiles/{}",
                    base_url, path.collection_id, path.tilematrixset_id
                ),
                hreflang: None,
                length: None,
            },
            Link {
                rel: "http://www.opengis.net/def/rel/ogc/1.0/tiling-scheme".to_string(),
                r#type: Some("application/json".to_string()),
                title: Some("TileMatrixSet".to_string()),
                href: format!("{}/api/tileMatrixSets/{}", base_url, path.tilematrixset_id),
                hreflang: None,
                length: None,
            },
            Link {
                rel: "item".to_string(),
                r#type: Some("application/vnd.mapbox-vector-tile".to_string()),
                title: Some("Mapbox Vector Tiles".to_string()),
                href: format!(
                    "{}/api/collections/{}/tiles/{}/{{tileMatrix}}/{{tileRow}}/{{tileCol}}",
                    base_url, path.collection_id, path.tilematrixset_id
                ),
                hreflang: None,
                length: None,
            },
        ],
        layers: tj.vector_layers.clone().map(|layers| {
            layers
                .into_iter()
                .map(|layer| GeospatialData {
                    title_description_keywords: TitleDescriptionKeywords {
                        title: None,
                        description: layer.description,
                        keywords: None,
                    },
                    id: layer.id,
                    data_type: DataType::Vector,
                    geometry_dimension: None,
                    feature_type: None,
                    point_of_contact: None,
                    publisher: None,
                    theme: None,
                    crs: None,
                    epoch: None,
                    min_scale_denominator: None,
                    max_scale_denominator: None,
                    min_cell_size: None,
                    max_cell_size: None,
                    max_tile_matrix: None,
                    min_tile_matrix: None,
                    bounding_box: None,
                    created: None,
                    updated: None,
                    style: None,
                    geo_data_classes: None,
                    properties_schema: None,
                    links: None,
                })
                .collect()
        }),
        bounding_box: tj.bounds.map(|b| BoundingBox2D {
            lower_left: [b.left, b.bottom],
            upper_right: [b.right, b.top],
            crs: None,
            ordered_axes: None,
        }),
        style: None,
        center_point: tj.center.map(|c| TilePoint {
            coordinates: Some([c.longitude, c.latitude]),
            crs: None,
            tile_matrix: Some(c.zoom.to_string()),
            scale_denominator: None,
            cell_size: None,
        }),
        license: None,
        access_constraints: None,
        version: None,
        created: None,
        updated: None,
        point_of_contact: None,
        media_types: Some(vec!["application/vnd.mapbox-vector-tile".to_string()]),
    };

    Ok(HttpResponse::Ok()
        .insert_header((CONTENT_TYPE, "application/json"))
        .json(tileset))
}

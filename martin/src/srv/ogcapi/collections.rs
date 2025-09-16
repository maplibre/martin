//! OGC API collections endpoints

use actix_web::http::header::CONTENT_TYPE;
use actix_web::web::{Data, Path};
use actix_web::{HttpRequest, HttpResponse, Result as ActixResult, route};
use log::warn;
use ogcapi_types::common::{Crs, Link};
use ogcapi_types::tiles::{
    BoundingBox2D, DataType, GeospatialData, TileMatrixLimits, TilePoint, TileSet, TileSetItem,
    TileSets, TitleDescriptionKeywords,
};
use serde::{Deserialize, Serialize};

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

/// OGC API Collection
#[derive(Serialize, Deserialize)]
pub struct Collection {
    pub id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub links: Vec<Link>,
    pub extent: Option<Extent>,
    pub crs: Vec<String>,
    #[serde(rename = "storageCrs")]
    pub storage_crs: Option<String>,
    #[serde(rename = "dataType")]
    pub data_type: Option<DataType>,
}

/// Spatial and temporal extent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Extent {
    pub spatial: Option<SpatialExtent>,
    pub temporal: Option<TemporalExtent>,
}

/// Spatial extent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpatialExtent {
    pub bbox: Vec<Vec<f64>>,
    pub crs: Option<String>,
}

/// Temporal extent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalExtent {
    pub interval: Vec<Vec<Option<String>>>,
}

/// OGC API Collections response
#[derive(Serialize, Deserialize)]
pub struct Collections {
    pub collections: Vec<Collection>,
    pub links: Vec<Link>,
}

impl Collections {
    pub fn from_catalog(_catalog: &Catalog, sources: &TileSources, base_url: &str) -> Self {
        // Get the list of source names from TileSources
        let collections = sources
            .source_names()
            .into_iter()
            .filter_map(|id| match sources.get_source(&id) {
                Ok(source) => {
                    let tj = source.get_tilejson();
                    Some(Collection {
                        id: id.clone(),
                        title: tj.name.clone(),
                        description: tj.description.clone(),
                        links: vec![
                            Link {
                                rel: "self".to_string(),
                                r#type: Some("application/json".to_string()),
                                title: Some(format!("Collection {id}")),
                                href: format!("{base_url}/api/collections/{id}"),
                                hreflang: None,
                                length: None,
                            },
                            Link {
                                rel: "http://www.opengis.net/def/rel/ogc/1.0/tilesets-vector"
                                    .to_string(),
                                r#type: Some("application/json".to_string()),
                                title: Some("Vector tilesets".to_string()),
                                href: format!("{base_url}/api/collections/{id}/tiles"),
                                hreflang: None,
                                length: None,
                            },
                        ],
                        extent: tj.bounds.map(|bounds| Extent {
                            spatial: Some(SpatialExtent {
                                bbox: vec![vec![
                                    bounds.left,
                                    bounds.bottom,
                                    bounds.right,
                                    bounds.top,
                                ]],
                                crs: Some(
                                    "http://www.opengis.net/def/crs/OGC/1.3/CRS84".to_string(),
                                ),
                            }),
                            temporal: None,
                        }),
                        crs: vec![
                            "http://www.opengis.net/def/crs/OGC/1.3/CRS84".to_string(),
                            "http://www.opengis.net/def/crs/EPSG/0/3857".to_string(),
                        ],
                        storage_crs: Some("http://www.opengis.net/def/crs/EPSG/0/3857".to_string()),
                        data_type: Some(DataType::Vector),
                    })
                }
                Err(e) => {
                    warn!("Failed to get source for collection '{}': {}", id, e);
                    None
                }
            })
            .collect();

        Self {
            collections,
            links: vec![Link {
                rel: "self".to_string(),
                r#type: Some("application/json".to_string()),
                title: Some("This document".to_string()),
                href: format!("{base_url}/api/collections"),
                hreflang: None,
                length: None,
            }],
        }
    }
}

/// OGC API Collections endpoint
#[route("/api/collections", method = "GET", method = "HEAD")]
pub async fn get_collections(
    req: HttpRequest,
    catalog: Data<Catalog>,
    sources: Data<TileSources>,
) -> ActixResult<HttpResponse> {
    let base_url = get_base_url(&req);
    let collections = Collections::from_catalog(&catalog, &sources, &base_url);

    Ok(HttpResponse::Ok()
        .insert_header((CONTENT_TYPE, "application/json"))
        .json(collections))
}

/// OGC API Collection endpoint
#[route("/api/collections/{collection_id}", method = "GET", method = "HEAD")]
pub async fn get_collection(
    req: HttpRequest,
    path: Path<CollectionPath>,
    catalog: Data<Catalog>,
    sources: Data<TileSources>,
) -> ActixResult<HttpResponse> {
    let base_url = get_base_url(&req);
    let collections = Collections::from_catalog(&catalog, &sources, &base_url);

    let collection = collections
        .collections
        .into_iter()
        .find(|c| c.id == path.collection_id)
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
                    geometry_dimension: None, // VectorLayer doesn't have geometry_type field
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

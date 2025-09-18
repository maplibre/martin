//! OGC API tilesets endpoint

use actix_web::http::header::CONTENT_TYPE;
use actix_web::web::Data;
use actix_web::{HttpRequest, HttpResponse, Result as ActixResult, route};
use log::warn;
use ogcapi_types::common::{Crs, Link};
use ogcapi_types::tiles::{DataType, TileSetItem, TileSets};

use super::landing::get_base_url;
use crate::source::TileSources;

/// Create `TileSets` response from catalog
pub fn create_tilesets(sources: &TileSources, base_url: &str) -> TileSets {
    let tilesets: Vec<TileSetItem> = sources
        .source_names()
        .into_iter()
        .filter_map(|id| match sources.get_source(&id) {
            Ok(source) => {
                let tj = source.get_tilejson();
                Some(TileSetItem {
                    title: tj.name.clone(),
                    data_type: DataType::Vector,
                    crs: Crs::from_epsg(3857),
                    tile_matrix_set_uri: Some(
                        "http://www.opengis.net/def/tilematrixset/OGC/1.0/WebMercatorQuad"
                            .to_string(),
                    ),
                    links: vec![
                        Link {
                            rel: "self".to_string(),
                            r#type: Some("application/json".to_string()),
                            title: Some(format!("Tileset for {id}")),
                            href: format!("{base_url}/api/collections/{id}/tiles/WebMercatorQuad"),
                            hreflang: None,
                            length: None,
                        },
                        Link {
                            rel: "http://www.opengis.net/def/rel/ogc/1.0/tileset".to_string(),
                            r#type: Some("application/json".to_string()),
                            title: Some("Tileset metadata".to_string()),
                            href: format!("{base_url}/api/collections/{id}/tiles/WebMercatorQuad"),
                            hreflang: None,
                            length: None,
                        },
                    ],
                })
            }
            Err(e) => {
                warn!("Failed to get source for tileset '{id}': {e}");
                None
            }
        })
        .collect();

    TileSets {
        tilesets,
        links: None,
    }
}

/// OGC API Tilesets endpoint
#[route("/api/tilesets", method = "GET", method = "HEAD")]
pub async fn get_tilesets(
    req: HttpRequest,
    sources: Data<TileSources>,
) -> ActixResult<HttpResponse> {
    let base_url = get_base_url(&req);
    let tilesets = create_tilesets(&sources, &base_url);

    Ok(HttpResponse::Ok()
        .insert_header((CONTENT_TYPE, "application/json"))
        .json(tilesets))
}

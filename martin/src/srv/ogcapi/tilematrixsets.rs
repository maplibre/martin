//! OGC API tile matrix sets endpoints

use std::num::{NonZeroU16, NonZeroU64};

use actix_web::http::header::CONTENT_TYPE;
use actix_web::web::Path;
use actix_web::{HttpRequest, HttpResponse, Result as ActixResult, route};
use martin_tile_utils::MAX_ZOOM;
use ogcapi_types::common::{Crs, Link};
use ogcapi_types::tiles::{
    CornerOfOrigin, TileMatrix, TileMatrixSet, TileMatrixSetItem, TileMatrixSets,
    TitleDescriptionKeywords,
};
use serde::Deserialize;

use super::landing::get_base_url;

#[derive(Deserialize)]
pub struct TileMatrixSetPath {
    tilematrixset_id: String,
}

/// Get default Web Mercator `TileMatrixSet`
pub fn get_web_mercator_tilematrixset() -> TileMatrixSet {
    const EARTH_RADIUS: f64 = 6378137.0;
    const ORIGIN_SHIFT: f64 = 2.0 * std::f64::consts::PI * EARTH_RADIUS / 2.0;

    let mut tile_matrices = Vec::new();

    // Generate tile matrices for zoom levels 0 to 22
    for zoom in 0..=MAX_ZOOM {
        let matrix_size = 1u64 << zoom;
        let pixel_size = (2.0 * ORIGIN_SHIFT) / (256.0 * matrix_size as f64);
        let scale_denominator = pixel_size * 0.00028; // 0.00028 meters per pixel

        tile_matrices.push(TileMatrix {
            title_description_keywords: TitleDescriptionKeywords {
                title: Some(format!("Zoom level {zoom}")),
                description: None,
                keywords: None,
            },
            id: zoom.to_string(),
            scale_denominator: scale_denominator / 0.00028,
            cell_size: pixel_size,
            corner_of_origin: Some(CornerOfOrigin::TopLeft),
            point_of_origin: [-ORIGIN_SHIFT, ORIGIN_SHIFT],
            tile_width: NonZeroU16::new(256).unwrap(),
            tile_height: NonZeroU16::new(256).unwrap(),
            matrix_width: NonZeroU64::new(matrix_size).unwrap(),
            matrix_height: NonZeroU64::new(matrix_size).unwrap(),
            variable_matrix_widths: None,
        });
    }

    TileMatrixSet {
        title_description_keywords: TitleDescriptionKeywords {
            title: Some("Web Mercator Quad".to_string()),
            description: Some("Standard Web Mercator tile matrix set".to_string()),
            keywords: None,
        },
        id: "WebMercatorQuad".to_string(),
        uri: Some("http://www.opengis.net/def/tilematrixset/OGC/1.0/WebMercatorQuad".to_string()),
        crs: Crs::from_epsg(3857),
        ordered_axes: Some(vec!["X".to_string(), "Y".to_string()]),
        well_known_scale_set: Some(
            "http://www.opengis.net/def/wkss/OGC/1.0/WebMercatorQuad".to_string(),
        ),
        bounding_box: None,
        tile_matrices,
    }
}

/// OGC API TileMatrixSets endpoint
#[route("/ogc/tileMatrixSets", method = "GET", method = "HEAD")]
pub async fn get_tile_matrix_sets(req: HttpRequest) -> ActixResult<HttpResponse> {
    let base_url = get_base_url(&req);
    let tilematrixsets = TileMatrixSets {
        tile_matrix_sets: vec![TileMatrixSetItem {
            id: Some("WebMercatorQuad".to_string()),
            title: Some("Web Mercator Quad".to_string()),
            uri: Some(
                "http://www.opengis.net/def/tilematrixset/OGC/1.0/WebMercatorQuad".to_string(),
            ),
            crs: Some(Crs::from_epsg(3857)),
            links: vec![
                Link::new(
                    format!("{base_url}/ogc/tileMatrixSets/WebMercatorQuad"),
                    "self",
                )
                .mediatype("application/json")
                .title("Web Mercator Quad TileMatrixSet"),
            ],
        }],
    };

    Ok(HttpResponse::Ok()
        .insert_header((CONTENT_TYPE, "application/json"))
        .json(tilematrixsets))
}

/// OGC API TileMatrixSet endpoint
#[route(
    "/ogc/tileMatrixSets/{tilematrixset_id}",
    method = "GET",
    method = "HEAD"
)]
pub async fn get_tile_matrix_set(path: Path<TileMatrixSetPath>) -> ActixResult<HttpResponse> {
    // For now, we only support WebMercatorQuad
    if path.tilematrixset_id != "WebMercatorQuad" {
        return Err(actix_web::error::ErrorNotFound("TileMatrixSet not found"));
    }

    let tilematrixset = get_web_mercator_tilematrixset();

    Ok(HttpResponse::Ok()
        .insert_header((CONTENT_TYPE, "application/json"))
        .json(tilematrixset))
}

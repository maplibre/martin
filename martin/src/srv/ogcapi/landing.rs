//! OGC API landing page endpoint

use actix_web::http::header::CONTENT_TYPE;
use actix_web::{HttpRequest, HttpResponse, Result as ActixResult, route};
use ogcapi_types::common::Link;
use serde::{Deserialize, Serialize};

/// OGC API Landing Page
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandingPage {
    pub title: String,
    pub description: Option<String>,
    pub links: Vec<Link>,
}

impl LandingPage {
    pub fn new(base_url: &str) -> Self {
        Self {
            title: "Martin Tile Server - OGC API".to_string(),
            description: Some("Access to Martin tile server via OGC API - Tiles".to_string()),
            links: vec![
                Link {
                    rel: "self".to_string(),
                    r#type: Some("application/json".to_string()),
                    title: Some("This document (JSON)".to_string()),
                    href: format!("{base_url}/api"),
                    hreflang: None,
                    length: None,
                },
                Link {
                    rel: "conformance".to_string(),
                    r#type: Some("application/json".to_string()),
                    title: Some("Conformance declaration".to_string()),
                    href: format!("{base_url}/api/conformance"),
                    hreflang: None,
                    length: None,
                },
                Link {
                    rel: "data".to_string(),
                    r#type: Some("application/json".to_string()),
                    title: Some("Collections".to_string()),
                    href: format!("{base_url}/api/collections"),
                    hreflang: None,
                    length: None,
                },
                Link {
                    rel: "http://www.opengis.net/def/rel/ogc/1.0/tilesets-vector".to_string(),
                    r#type: Some("application/json".to_string()),
                    title: Some("Vector tilesets".to_string()),
                    href: format!("{base_url}/api/tilesets"),
                    hreflang: None,
                    length: None,
                },
                Link {
                    rel: "http://www.opengis.net/def/rel/ogc/1.0/tilesets-map".to_string(),
                    r#type: Some("application/json".to_string()),
                    title: Some("Map tilesets".to_string()),
                    href: format!("{base_url}/api/tilesets"),
                    hreflang: None,
                    length: None,
                },
                Link {
                    rel: "http://www.opengis.net/def/rel/ogc/1.0/tiling-schemes".to_string(),
                    r#type: Some("application/json".to_string()),
                    title: Some("Tiling schemes".to_string()),
                    href: format!("{base_url}/api/tileMatrixSets"),
                    hreflang: None,
                    length: None,
                },
            ],
        }
    }
}

/// Get base URL from request
pub fn get_base_url(req: &HttpRequest) -> String {
    let connection_info = req.connection_info();
    let scheme = connection_info.scheme();
    let host = connection_info.host();
    format!("{scheme}://{host}")
}

/// OGC API Landing Page endpoint
#[route("/api", method = "GET", method = "HEAD")]
pub async fn get_landing_page(req: HttpRequest) -> ActixResult<HttpResponse> {
    let base_url = get_base_url(&req);
    let landing_page = LandingPage::new(&base_url);

    Ok(HttpResponse::Ok()
        .insert_header((CONTENT_TYPE, "application/json"))
        .json(landing_page))
}

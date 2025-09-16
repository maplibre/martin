//! OGC API conformance endpoint

use actix_web::http::header::CONTENT_TYPE;
use actix_web::{HttpResponse, Result as ActixResult, route};
use serde::{Deserialize, Serialize};

/// OGC API Conformance response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conformance {
    #[serde(rename = "conformsTo")]
    pub conforms_to: Vec<String>,
}

impl Conformance {
    pub fn new() -> Self {
        Self {
            conforms_to: vec![
                // Core
                "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/core".to_string(),
                // Tileset
                "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/tileset".to_string(),
                // TileJSON
                "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/tilejson".to_string(),
                // Collections
                "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/collections".to_string(),
                // Dataset tilesets
                "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/dataset-tilesets".to_string(),
                // Mapbox Vector Tiles
                "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/mvt".to_string(),
                // PNG
                "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/png".to_string(),
                // JPEG
                "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/jpeg".to_string(),
                // WebP
                "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/webp".to_string(),
            ],
        }
    }
}

impl Default for Conformance {
    fn default() -> Self {
        Self::new()
    }
}

/// OGC API Conformance endpoint
#[route("/api/conformance", method = "GET", method = "HEAD")]
pub async fn get_conformance() -> ActixResult<HttpResponse> {
    let conformance = Conformance::new();

    Ok(HttpResponse::Ok()
        .insert_header((CONTENT_TYPE, "application/json"))
        .json(conformance))
}

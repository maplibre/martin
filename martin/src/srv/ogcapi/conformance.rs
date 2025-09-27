//! OGC API conformance endpoint

use actix_web::{HttpResponse, route};
use ogcapi_types::common::Conformance;

/// OGC API Conformance endpoint
#[route("/api/conformance", method = "GET", method = "HEAD")]
pub async fn get_conformance() -> HttpResponse {
    HttpResponse::Ok().json(Conformance::new(&[
        // Tiles Core "we can serve tiles"
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/core",
        // Tileset
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/tileset",
        // TileJSON
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/tilejson",
        // Collections
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/collections",
        // Dataset tilesets
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/dataset-tilesets",
        // formats we serve
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/mvt",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/png",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/jpeg",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/geojson",
        // webp + gif don't have conformance classes
    ]))
}

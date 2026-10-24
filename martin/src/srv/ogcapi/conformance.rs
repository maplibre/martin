use actix_middleware_etag::Etag;
use actix_web::middleware::Compress;
use actix_web::{HttpResponse, route};
use serde::Serialize;

/// The conformance classes of OGC API - Common, OGC API - Tiles and the Tile Matrix Set standard this API implements.
const CONFORMS_TO: [&str; 10] = [
    "http://www.opengis.net/spec/ogcapi-common-1/1.0/conf/core",
    "http://www.opengis.net/spec/ogcapi-common-1/1.0/conf/landing-page",
    "http://www.opengis.net/spec/ogcapi-common-1/1.0/conf/json",
    "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/core",
    "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/tileset",
    "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/tilesets-list",
    "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/geodata-tilesets",
    "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/mvt",
    "http://www.opengis.net/spec/tms/2.0/conf/tilematrixset",
    "http://www.opengis.net/spec/tms/2.0/conf/json-tilematrixset",
];

/// The conformance declaration of the OGC API.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Conformance {
    conforms_to: &'static [&'static str],
}

/// The conformance classes the OGC API implements.
#[route(
    "/ogc/conformance",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
pub async fn get_conformance() -> HttpResponse {
    HttpResponse::Ok().json(Conformance {
        conforms_to: &CONFORMS_TO,
    })
}

use actix_middleware_etag::Etag;
use actix_web::middleware::Compress;
use actix_web::{HttpResponse, route};
use serde::Serialize;

/// The conformance classes of OGC API - Common, OGC API - Tiles and the Tile Matrix Set standard this API implements.
const TILES_CONFORMS_TO: [&str; 10] = [
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

/// The conformance classes of OGC API - Styles this API implements.
#[cfg(feature = "resources")]
const STYLES_CONFORMS_TO: [&str; 3] = [
    "http://www.opengis.net/spec/ogcapi-styles-1/1.0/conf/core",
    "http://www.opengis.net/spec/ogcapi-styles-1/1.0/conf/global-styles",
    "http://www.opengis.net/spec/ogcapi-styles-1/1.0/conf/mapbox-style",
];

/// The conformance declaration of the OGC API.
#[derive(Serialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
struct Conformance {
    conforms_to: Vec<&'static str>,
}

/// The conformance classes the OGC API implements.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/conformance",
        responses(
            (status = 200, description = "Conformance classes the OGC API implements", body = Conformance),
        ),
    )
)]
#[route(
    "/ogc/conformance",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
pub async fn get_conformance() -> HttpResponse {
    #[cfg_attr(not(feature = "resources"), expect(unused_mut, reason = "only styles add classes"))]
    let mut conforms_to = TILES_CONFORMS_TO.to_vec();
    #[cfg(feature = "resources")]
    conforms_to.extend(STYLES_CONFORMS_TO);
    HttpResponse::Ok().json(Conformance { conforms_to })
}

//! OGC API landing page endpoint

use actix_web::http::header::CONTENT_TYPE;
use actix_web::{HttpRequest, HttpResponse, Result as ActixResult, route};
use ogcapi_types::common::{LandingPage, Link};

/// Create the OGC API Landing Page
fn create_landing_page(base_url: &str) -> LandingPage {
    let links = vec![
        Link::new(format!("{base_url}/ogc/"), "self")
            .mediatype("application/json")
            .title("Landing page"),
        Link::new(format!("{base_url}/ogc/conformance"), "conformance")
            .mediatype("application/json")
            .title("Conformance declaration"),
        Link::new(format!("{base_url}/ogc/collections"), "data")
            .mediatype("application/json")
            .title("Collections"),
        Link::new(
            format!("{base_url}/ogc/tilesets"),
            "http://www.opengis.net/def/rel/ogc/1.0/tilesets-vector",
        )
        .mediatype("application/json")
        .title("Vector tilesets"),
        Link::new(
            format!("{base_url}/ogc/tilesets"),
            "http://www.opengis.net/def/rel/ogc/1.0/tilesets-map",
        )
        .mediatype("application/json")
        .title("Map tilesets"),
        Link::new(
            format!("{base_url}/ogc/tileMatrixSets"),
            "http://www.opengis.net/def/rel/ogc/1.0/tiling-schemes",
        )
        .mediatype("application/json")
        .title("Tile matrix sets"),
    ];

    LandingPage::new("Martin Tile Server - OGC API")
        .title("Martin Tile Server - OGC API")
        .description("Access to Martin tile server via OGC API - Tiles")
        .links(links)
}

/// Get base URL from request
#[must_use]
pub fn get_base_url(req: &HttpRequest) -> String {
    let connection_info = req.connection_info();
    let scheme = connection_info.scheme();
    let host = connection_info.host();
    format!("{scheme}://{host}")
}

/// OGC API Landing Page endpoint
#[route("/ogc/", method = "GET", method = "HEAD")]
pub async fn get_landing_page(req: HttpRequest) -> ActixResult<HttpResponse> {
    let base_url = get_base_url(&req);
    let landing_page = create_landing_page(&base_url);

    Ok(HttpResponse::Ok()
        .insert_header((CONTENT_TYPE, "application/json"))
        .json(landing_page))
}

use actix_middleware_etag::Etag;
use actix_web::middleware::Compress;
use actix_web::web::Data;
use actix_web::{HttpRequest, HttpResponse, route};
use serde::Serialize;

use super::link::{Link, ogc_url};
use crate::config::file::srv::SrvConfig;

/// The entry point of the OGC API.
#[derive(Serialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
struct LandingPage {
    title: &'static str,
    description: &'static str,
    links: Vec<Link>,
}

/// The OGC API landing page, at `/ogc` and `/ogc/`.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc",
        responses(
            (status = 200, description = "OGC API landing page", body = LandingPage),
        ),
    )
)]
#[route(
    "/ogc{trailing_slash:/?}",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
pub async fn get_landing_page(req: HttpRequest, srv_config: Data<SrvConfig>) -> HttpResponse {
    HttpResponse::Ok().json(LandingPage {
        title: "Martin",
        description: "The tiles Martin serves, as an OGC API",
        links: vec![
            Link::json(ogc_url(&req, &srv_config, ""), "self", "This document"),
            Link {
                href: "https://maplibre.org/martin/using/".to_owned(),
                rel: "service-doc",
                media_type: "text/html",
                title: "The Martin HTTP API",
                templated: false,
            },
            Link::json(
                ogc_url(&req, &srv_config, "/conformance"),
                "conformance",
                "Conformance classes",
            ),
            Link::json(ogc_url(&req, &srv_config, "/collections"), "data", "Collections"),
            Link::json(
                ogc_url(&req, &srv_config, "/tileMatrixSets"),
                "http://www.opengis.net/def/rel/ogc/1.0/tiling-schemes",
                "Tile matrix sets",
            ),
        ],
    })
}

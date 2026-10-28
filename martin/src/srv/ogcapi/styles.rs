use actix_middleware_etag::Etag;
use actix_web::middleware::Compress;
use actix_web::web::Data;
use actix_web::{HttpRequest, HttpResponse, route};
use martin_core::styles::StyleSources;
use serde::Serialize;

use super::link::{Link, ogc_url, path_segment};
use crate::config::file::srv::SrvConfig;
use crate::maplibre_style::Style;

/// The styles the OGC API serves.
#[derive(Serialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
struct Styles {
    styles: Vec<StyleDescription>,
    links: Vec<Link>,
}

/// A style in the list, linking to its stylesheets.
#[derive(Serialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
struct StyleDescription {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    links: Vec<Link>,
}

/// Style `id` as listed under `/styles`, titled after its `name`.
async fn style_description(
    req: &HttpRequest,
    srv_config: &SrvConfig,
    id: &str,
    path: &std::path::Path,
) -> StyleDescription {
    let title = tokio::fs::read_to_string(path)
        .await
        .ok()
        .and_then(|content| serde_json::from_str::<Style>(&content).ok())
        .and_then(|style| style.other.get("name")?.as_str().map(ToOwned::to_owned));
    #[cfg_attr(
        not(all(feature = "rendering", target_os = "linux")),
        expect(unused_mut, reason = "only maps add links")
    )]
    let mut links = vec![Link {
        href: ogc_url(req, srv_config, &format!("/styles/{}", path_segment(id))),
        rel: "stylesheet",
        media_type: "application/json",
        title: "MapLibre style",
        templated: false,
    }];
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    if super::map::renders_maps(req) {
        links.extend(super::map::map_links(
            &ogc_url(req, srv_config, &format!("/styles/{}/map", path_segment(id))),
            "Map of the style",
        ));
        links.push(Link::json(
            ogc_url(req, srv_config, &format!("/styles/{}/map/tiles", path_segment(id))),
            super::data_type::DataType::Raster.tilesets_rel(),
            "Map tilesets of the style",
        ));
    }
    StyleDescription {
        id: id.to_owned(),
        title,
        links,
    }
}

/// Every style Martin serves.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/styles",
        operation_id = "ogc.dataset.getStyles",
        responses(
            (status = 200, description = "Every style Martin serves", body = Styles),
        ),
    )
)]
#[route(
    "/ogc/styles",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
pub async fn get_styles(req: HttpRequest, srv_config: Data<SrvConfig>, styles: Data<StyleSources>) -> HttpResponse {
    let mut descriptions = Vec::new();
    for (id, entry) in styles.get_catalog() {
        descriptions.push(style_description(&req, &srv_config, &id, &entry.path).await);
    }
    HttpResponse::Ok().json(Styles {
        styles: descriptions,
        links: vec![Link::json(
            ogc_url(&req, &srv_config, "/styles"),
            "self",
            "This document",
        )],
    })
}

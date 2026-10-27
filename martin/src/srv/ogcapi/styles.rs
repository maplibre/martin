use actix_middleware_etag::Etag;
use actix_web::middleware::Compress;
use actix_web::web::{Data, Path, Query};
use actix_web::{HttpRequest, HttpResponse, Result as ActixResult, route};
use martin_core::styles::StyleSources;
use serde::{Deserialize, Serialize};

use super::error::OgcApiError;
use super::link::{Link, ogc_url, path_segment};
use crate::config::file::srv::SrvConfig;
use crate::maplibre_style::Style;
use crate::srv::server::map_error;
use crate::srv::styles::absolute_style;

/// The value of `f` that selects the `MapLibre` style encoding.
const MAPLIBRE_STYLE_FORMAT: &str = "json";

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

/// The query of a style request.
#[derive(Deserialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::IntoParams))]
pub struct StyleQuery {
    /// Style encoding, only `json` is supported.
    f: Option<String>,
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
    StyleDescription {
        id: id.to_owned(),
        title,
        links: vec![Link {
            href: ogc_url(req, srv_config, &format!("/styles/{}", path_segment(id))),
            rel: "stylesheet",
            media_type: "application/json",
            title: "MapLibre style",
            templated: false,
        }],
    }
}

/// Every style Martin serves.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/styles",
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

/// One style in the `MapLibre` style format.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/styles/{style_id}",
        params(("style_id" = String, Path, description = "Style ID"), StyleQuery),
        responses(
            (status = 200, description = "MapLibre style", content_type = "application/json"),
            (status = 400, description = "Unsupported style encoding, or the style file is malformed"),
            (status = 404, description = "No such style"),
        ),
    )
)]
#[route(
    "/ogc/styles/{style_id}",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
pub async fn get_style(
    req: HttpRequest,
    path: Path<String>,
    query: Query<StyleQuery>,
    srv_config: Data<SrvConfig>,
    styles: Data<StyleSources>,
) -> ActixResult<HttpResponse> {
    let id = path.into_inner();
    if let Some(format) = query.into_inner().f.filter(|format| format != MAPLIBRE_STYLE_FORMAT) {
        return Err(map_error(&OgcApiError::UnsupportedStyleEncoding { encoding: format }));
    }
    Ok(match absolute_style(&req, &srv_config, &styles, &id).await {
        Ok(style) => HttpResponse::Ok().json(style),
        Err(unavailable) => unavailable.response(),
    })
}

use actix_middleware_etag::Etag;
use actix_web::http::header::ACCEPT;
use actix_web::middleware::Compress;
use actix_web::web::{Data, Query};
use actix_web::{HttpRequest, HttpResponse, Result as ActixResult, route};
use serde::Deserialize;
use serde_json::{Value, json};

use super::error::OgcApiError;
use super::link::ogc_url;
use crate::config::file::srv::SrvConfig;
use crate::srv::server::map_error;

/// Media type of an `OpenAPI` 3.1 definition in JSON.
pub(super) const OPENAPI_JSON: &str = "application/vnd.oai.openapi+json;version=3.1";

/// Martin's `OpenAPI` definition, as `just gen-schemas` writes it.
const DEFINITION: &str = include_str!("../../../openapi.json");

/// Swagger UI showing the definition this page is served next to, as `?f=json`.
const API_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Martin OGC API</title>
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5.17.14/swagger-ui.css">
</head>
<body>
<div id="swagger-ui"></div>
<script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5.17.14/swagger-ui-bundle.js"></script>
<script>SwaggerUIBundle({ url: "?f=json", dom_id: "#swagger-ui" });</script>
</body>
</html>
"##;

/// The query of an API definition request.
#[derive(Deserialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::IntoParams))]
pub struct ApiQuery {
    /// Encoding of the definition, `json` or `html`; `html` when not given and the client accepts HTML.
    f: Option<String>,
}

/// The OGC API part of Martin's `OpenAPI` 3.1 definition, as JSON or as an HTML page showing it.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/api",
        operation_id = "getAPI",
        params(ApiQuery),
        responses(
            (status = 200, description = "OpenAPI 3.1 definition of the OGC API paths this server answers, or an HTML page showing it", content(
                ("application/vnd.oai.openapi+json;version=3.1"),
                ("text/html"),
            )),
            (status = 400, description = "Unsupported encoding"),
        ),
    )
)]
#[route(
    "/ogc/api",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
pub async fn get_api(
    req: HttpRequest,
    query: Query<ApiQuery>,
    srv_config: Data<SrvConfig>,
) -> ActixResult<HttpResponse> {
    let html = match query.into_inner().f.as_deref() {
        Some("json") => false,
        Some("html") => true,
        Some(encoding) => {
            return Err(map_error(&OgcApiError::UnsupportedApiEncoding {
                encoding: encoding.to_owned(),
            }));
        }
        None => req
            .headers()
            .get(ACCEPT)
            .and_then(|accept| accept.to_str().ok())
            .is_some_and(|accept| accept.contains("text/html")),
    };
    Ok(if html {
        HttpResponse::Ok()
            .content_type("text/html; charset=utf-8")
            .body(API_HTML)
    } else {
        HttpResponse::Ok()
            .content_type(OPENAPI_JSON)
            .json(definition(&req, &srv_config))
    })
}

/// The OGC API paths of Martin's `OpenAPI` definition that `req`'s server serves, relative to its landing page.
fn definition(req: &HttpRequest, srv_config: &SrvConfig) -> Value {
    let mut definition: Value = serde_json::from_str(DEFINITION).expect("martin/openapi.json is JSON");
    definition["servers"] = json!([{ "url": ogc_url(req, srv_config, "") }]);
    if let Some(paths) = definition["paths"].as_object_mut() {
        *paths = std::mem::take(paths)
            .into_iter()
            .filter(|(_, item)| {
                item.get("x-martin-feature")
                    .and_then(Value::as_str)
                    .is_none_or(|feature| serves(req, feature))
            })
            .filter_map(|(path, item)| match path.strip_prefix("/ogc")? {
                "" => Some(("/".to_owned(), item)),
                ogc_path if ogc_path.starts_with('/') => Some((ogc_path.to_owned(), item)),
                _ => None,
            })
            .collect();
    }
    definition
}

/// Whether the server answering `req` serves the paths `x-martin-feature` marks with `feature`.
fn serves(req: &HttpRequest, feature: &str) -> bool {
    match feature {
        "resources" => cfg!(feature = "resources"),
        "rendering" => renders_styles(req),
        _ => true,
    }
}

/// Whether the server answering `req` renders styles, built with rendering and with it enabled.
#[cfg(all(feature = "rendering", target_os = "linux"))]
fn renders_styles(req: &HttpRequest) -> bool {
    req.app_data::<Data<martin_core::styles::StyleSources>>()
        .is_some_and(|styles| styles.is_rendering_enabled())
}

/// Whether the server answering `req` renders styles, which a build without rendering never does.
#[cfg(not(all(feature = "rendering", target_os = "linux")))]
fn renders_styles(_req: &HttpRequest) -> bool {
    false
}

use actix_middleware_etag::Etag;
use actix_web::http::Uri;
use actix_web::http::header::{ContentType, LOCATION};
use actix_web::middleware::Compress;
use actix_web::web::{Data, Path};
use actix_web::{HttpRequest, HttpResponse, route};
use futures::{StreamExt as _, stream};
use martin_core::styles::StyleSources;
use serde::Deserialize;
use tracing::{error, instrument, warn};

use crate::config::file::srv::SrvConfig;
use crate::maplibre_style::{Style, merge_styles};
use crate::srv::server::DebouncedWarning;

/// Same limit as composite tile requests.
const MAX_STYLE_IDS_PER_REQUEST: usize = 128;
/// Style files read from disk at once for one request.
const MAX_CONCURRENT_STYLE_READS: usize = 16;

#[derive(Deserialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "unstable-schemas", into_params(parameter_in = Path))]
struct StyleRequest {
    /// One style ID, or up to 128 comma-separated style IDs to merge in order.
    style_id: String,
}

#[derive(Debug)]
enum LoadStyleError {
    NotFound,
    Malformed {
        style_id: String,
        path: std::path::PathBuf,
        error: serde_json::Error,
    },
}

#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/style/{style_id}",
        params(StyleRequest),
        responses(
            (status = 200, description = "MapLibre Style Spec JSON document", content_type = "application/json"),
            (status = 400, description = "Style file is malformed or styles cannot be merged"),
            (status = 404, description = "No matching style"),
        ),
    )
)]
#[route(
    "/style/{style_id}",
    method = "GET",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
#[hotpath::measure]
#[instrument(level = "debug", skip_all, fields(style.id = %path.style_id))]
pub async fn get_style_json(
    req: HttpRequest,
    path: Path<StyleRequest>,
    styles: Data<StyleSources>,
    srv_config: Data<SrvConfig>,
) -> HttpResponse {
    let style_ids: Vec<&str> = path.style_id.split(',').map(str::trim).collect();
    if style_ids.iter().any(|id| id.is_empty()) {
        return HttpResponse::BadRequest()
            .content_type(ContentType::plaintext())
            .body("Style ids must not be empty");
    }
    if style_ids.len() > MAX_STYLE_IDS_PER_REQUEST {
        return HttpResponse::BadRequest()
            .content_type(ContentType::plaintext())
            .body(format!(
                "Requested {} style ids, but at most {MAX_STYLE_IDS_PER_REQUEST} are allowed per request",
                style_ids.len()
            ));
    }

    // MapLibre clients don't consistently support relative URLs. Build an
    // absolute base URL using the same prefix precedence as TileJSON URLs.
    let prefix = path_prefix(&req, &srv_config);
    let base_url = {
        let info = req.connection_info();
        format!("{}://{}{prefix}", info.scheme(), info.host())
    };

    let loaded = stream::iter(style_ids.into_iter().map(|style_id| {
        let styles = &styles;
        let base_url = &base_url;
        async move {
            let Some(path) = styles.style_json_path(style_id) else {
                return Err(LoadStyleError::NotFound);
            };
            let Ok(style_content) = tokio::fs::read_to_string(&path).await else {
                // The file was likely deleted after Martin collected its file list.
                return Err(LoadStyleError::NotFound);
            };
            let mut style = serde_json::from_str::<Style>(&style_content).map_err(|error| {
                LoadStyleError::Malformed {
                    style_id: style_id.to_owned(),
                    path,
                    error,
                }
            })?;
            style.expand_relative_urls(base_url);
            Ok((style_id.to_owned(), style))
        }
    }))
    .buffered(MAX_CONCURRENT_STYLE_READS)
    .collect::<Vec<_>>()
    .await;

    let mut parsed = Vec::with_capacity(loaded.len());
    for result in loaded {
        match result {
            Ok(style) => parsed.push(style),
            Err(LoadStyleError::NotFound) => {
                return HttpResponse::NotFound()
                    .content_type(ContentType::plaintext())
                    .body("No such style exists");
            }
            Err(LoadStyleError::Malformed {
                style_id,
                path,
                error: e,
            }) => {
                error!(
                    "Failed to parse style JSON {e:?} for style {style_id} at {:?}",
                    path.display()
                );
                return HttpResponse::BadRequest()
                    .content_type(ContentType::plaintext())
                    .body(format!(
                        "The requested style {style_id} is malformed: {e:?}"
                    ));
            }
        }
    }

    if parsed.len() == 1 {
        return HttpResponse::Ok().json(parsed.pop().expect("one style was loaded").1);
    }
    match merge_styles(parsed, &base_url) {
        Ok(style) => HttpResponse::Ok().json(style),
        Err(error) => HttpResponse::BadRequest()
            .content_type(ContentType::plaintext())
            .body(error.to_string()),
    }
}

/// Resolve the URL path prefix under which Martin is publicly served.
///
/// Returns an empty string when no prefix applies, otherwise a leading-slash
/// path with no trailing slash (e.g. `/tiles`).
///
/// Note: `X-Rewrite-URL` is intentionally not honored here. Unlike the
/// `TileJSON` case where the header's full path can be used directly, for
/// styles the header would contain the full style request path
/// (e.g. `/tiles/style/foo/style.json`), which isn't a usable prefix.
fn path_prefix(req: &HttpRequest, srv_config: &SrvConfig) -> String {
    let Some(prefix) = srv_config.public_path_prefix() else {
        return req
            .headers()
            .get("X-Forwarded-Prefix")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<Uri>().ok())
            .map(|v| v.path().trim_end_matches('/').to_owned())
            .unwrap_or_default();
    };
    prefix.to_owned()
}

/// Redirect `/styles/{style_id}` to `/style/{style_id}` (HTTP 301)
/// This handles common pluralization mistakes
#[route("/styles/{style_id}", method = "GET", method = "HEAD")]
pub(crate) async fn redirect_styles(path: Path<StyleRequest>) -> HttpResponse {
    static WARNING: DebouncedWarning = DebouncedWarning::new();
    let StyleRequest { style_id } = path.as_ref();
    WARNING
        .once_per_hour(|| {
            warn!(
                "Request to /styles/{style_id} caused unnecessary redirect. Use /style/{style_id} to avoid extra round-trip latency."
            );
        })
        .await;

    HttpResponse::MovedPermanently()
        .insert_header((LOCATION, format!("/style/{style_id}")))
        .finish()
}

#[cfg(test)]
mod tests {
    use actix_web::test::TestRequest;

    use super::*;

    fn cfg(base_path: Option<&str>, route_prefix: Option<&str>) -> SrvConfig {
        SrvConfig {
            base_path: base_path.map(str::to_owned),
            route_prefix: route_prefix.map(str::to_owned),
            ..Default::default()
        }
    }

    #[test]
    fn path_prefix_empty_when_nothing_configured() {
        let req = TestRequest::default().to_http_request();
        assert_eq!(path_prefix(&req, &cfg(None, None)), "");
    }

    #[test]
    fn path_prefix_uses_base_path_first() {
        let req = TestRequest::default()
            .insert_header(("X-Forwarded-Prefix", "/header"))
            .to_http_request();
        assert_eq!(
            path_prefix(&req, &cfg(Some("/from_base"), Some("/from_route"))),
            "/from_base"
        );
    }

    #[test]
    fn path_prefix_falls_back_to_route_prefix() {
        let req = TestRequest::default()
            .insert_header(("X-Forwarded-Prefix", "/header"))
            .to_http_request();
        assert_eq!(
            path_prefix(&req, &cfg(None, Some("/from_route"))),
            "/from_route"
        );
    }

    #[test]
    fn path_prefix_falls_back_to_forwarded_prefix_header() {
        let req = TestRequest::default()
            .insert_header(("X-Forwarded-Prefix", "/from_header"))
            .to_http_request();
        assert_eq!(path_prefix(&req, &cfg(None, None)), "/from_header");
    }

    #[test]
    fn path_prefix_strips_trailing_slash_from_header() {
        let req = TestRequest::default()
            .insert_header(("X-Forwarded-Prefix", "/from_header/"))
            .to_http_request();
        assert_eq!(path_prefix(&req, &cfg(None, None)), "/from_header");
    }
}

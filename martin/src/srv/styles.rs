use actix_middleware_etag::Etag;
use actix_web::http::Uri;
use actix_web::http::header::{ContentType, LOCATION};
use actix_web::middleware::Compress;
use actix_web::web::{Data, Path};
use actix_web::{HttpRequest, HttpResponse, route};
use martin_core::styles::StyleSources;
use serde::Deserialize;
use tracing::{error, instrument, warn};

use crate::config::file::srv::SrvConfig;
use crate::maplibre_style::Style;
use crate::srv::server::DebouncedWarning;

#[derive(Deserialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "unstable-schemas", into_params(parameter_in = Path))]
struct StyleRequest {
    style_id: String,
}

#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/style/{style_id}",
        params(StyleRequest),
        responses(
            (status = 200, description = "MapLibre Style Spec JSON document", content_type = "application/json"),
            (status = 400, description = "Style file is malformed"),
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
    let style_id = &path.style_id;
    let Some(path) = styles.style_json_path(style_id) else {
        return HttpResponse::NotFound()
            .content_type(ContentType::plaintext())
            .body("No such style exists");
    };
    let Ok(style_content) = tokio::fs::read_to_string(&path).await else {
        // the file was likely deleted after martin was launched and collected the file list
        // TODO: change this to a server error and log appropriately once the watch mode is here
        return HttpResponse::NotFound()
            .content_type(ContentType::plaintext())
            .body("No such style exists");
    };
    match serde_json::from_str::<Style>(&style_content) {
        Ok(mut style) => {
            // maplibre clients don't fully support relative URLs
            // At the time of writing:
            //   - maplibre-gl-js supports relative URLs for tilesets and sprites (but not glyphs)
            //   - maplibre-native doesn't seem to support relative URLs at all
            //
            // Build an absolute base URL using the request's scheme/host and the
            // configured path prefix, mirroring the precedence used for TileJSON
            // URLs in `srv/tiles/metadata.rs`:
            //   base_path > route_prefix > X-Forwarded-Prefix > ""
            let prefix = path_prefix(&req, &srv_config);
            let info = req.connection_info();
            let base_url = format!("{}://{}{prefix}", info.scheme(), info.host());
            style.expand_relative_urls(&base_url);
            HttpResponse::Ok().json(style)
        }
        Err(e) => {
            error!(
                "Failed to parse style JSON {e:?} for style {style_id} at {:?}",
                path.display()
            );

            HttpResponse::BadRequest()
                .content_type(ContentType::plaintext())
                .body(format!(
                    "The requested style {style_id} is malformed: {e:?}"
                ))
        }
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
    if let Some(prefix) = srv_config.public_path_prefix() {
        prefix.to_string()
    } else {
        req.headers()
            .get("X-Forwarded-Prefix")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<Uri>().ok())
            .map(|v| v.path().trim_end_matches('/').to_string())
            .unwrap_or_default()
    }
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
            base_path: base_path.map(ToString::to_string),
            route_prefix: route_prefix.map(ToString::to_string),
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

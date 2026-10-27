use actix_middleware_etag::Etag;
use actix_web::http::header::{ContentType, LOCATION};
use actix_web::middleware::Compress;
use actix_web::web::{Data, Path};
use actix_web::{HttpRequest, HttpResponse, route};
use martin_core::styles::StyleSources;
use serde::Deserialize;
use tracing::{error, instrument, warn};

use crate::config::file::srv::SrvConfig;
use crate::maplibre_style::Style;
use crate::srv::server::{DebouncedWarning, path_prefix};

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
    method = "HEAD",
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
    match absolute_style(&req, &srv_config, &styles, &path.style_id).await {
        Ok(style) => HttpResponse::Ok().json(style),
        Err(unavailable) => unavailable.response(),
    }
}

/// Why a style can't be served.
pub(crate) enum StyleUnavailable {
    /// No style has this ID, or its file is gone.
    Missing,
    /// The style file isn't a valid style.
    Malformed {
        /// ID of the style.
        style_id: String,
        /// Why the file doesn't parse.
        error: serde_json::Error,
    },
}

impl StyleUnavailable {
    /// The plain-text response telling the client why the style can't be served.
    pub(crate) fn response(&self) -> HttpResponse {
        match self {
            Self::Missing => HttpResponse::NotFound()
                .content_type(ContentType::plaintext())
                .body("No such style exists"),
            Self::Malformed { style_id, error } => HttpResponse::BadRequest()
                .content_type(ContentType::plaintext())
                .body(format!("The requested style {style_id} is malformed: {error:?}")),
        }
    }
}

/// Style `style_id` with its relative URLs made absolute against the URL `req` came in on.
pub(crate) async fn absolute_style(
    req: &HttpRequest,
    srv_config: &SrvConfig,
    styles: &StyleSources,
    style_id: &str,
) -> Result<Style, StyleUnavailable> {
    let Some(path) = styles.style_json_path(style_id) else {
        return Err(StyleUnavailable::Missing);
    };
    let Ok(style_content) = tokio::fs::read_to_string(&path).await else {
        // the file was likely deleted after martin was launched and collected the file list
        // TODO: change this to a server error and log appropriately once the watch mode is here
        return Err(StyleUnavailable::Missing);
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
            let prefix = path_prefix(req, srv_config);
            let info = req.connection_info();
            let base_url = format!("{}://{}{prefix}", info.scheme(), info.host());
            style.expand_relative_urls(&base_url);
            Ok(style)
        }
        Err(e) => {
            error!(
                style.id = %style_id,
                style.path = %path.display(),
                error = ?e,
                "Failed to parse style JSON"
            );

            Err(StyleUnavailable::Malformed {
                style_id: style_id.to_owned(),
                error: e,
            })
        }
    }
}

/// Redirect `/styles/{style_id}` to `/style/{style_id}` (HTTP 301)
/// This handles common pluralization mistakes
#[route("/styles/{style_id}", method = "GET", method = "HEAD")]
pub async fn redirect_styles(path: Path<StyleRequest>) -> HttpResponse {
    static WARNING: DebouncedWarning = DebouncedWarning::new();
    let StyleRequest { style_id } = path.as_ref();
    WARNING
        .once_per_hour(|| {
            warn!(
                style.id = %style_id,
                "Request to /styles caused unnecessary redirect. Use /style to avoid extra round-trip latency."
            );
        })
        .await;

    HttpResponse::MovedPermanently()
        .insert_header((LOCATION, format!("/style/{style_id}")))
        .finish()
}

use actix_middleware_etag::Etag;
use actix_web::http::header::{ContentType, LOCATION};
use actix_web::middleware::Compress;
use actix_web::web::{Data, Path};
use actix_web::{HttpRequest, HttpResponse, route, routes};
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
#[routes]
#[get("/style/{style_id}", wrap = "Etag::default()", wrap = "Compress::default()")]
#[head("/style/{style_id}", wrap = "Etag::default()", wrap = "Compress::default()")]
#[cfg_attr(
    feature = "ogcapi",
    get("/ogc/styles/{style_id}", wrap = "Etag::default()", wrap = "Compress::default()")
)]
#[cfg_attr(
    feature = "ogcapi",
    head("/ogc/styles/{style_id}", wrap = "Etag::default()", wrap = "Compress::default()")
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
                style.id = %style_id,
                style.path = %path.display(),
                error = ?e,
                "Failed to parse style JSON"
            );

            HttpResponse::BadRequest()
                .content_type(ContentType::plaintext())
                .body(format!("The requested style {style_id} is malformed: {e:?}"))
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

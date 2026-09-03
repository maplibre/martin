use std::sync::Arc;

use actix_middleware_etag::Etag;
use actix_web::http::header::LOCATION;
use actix_web::middleware::Compress;
use actix_web::web::{Data, Path};
use actix_web::{HttpResponse, Result as ActixResult, route, routes};
use martin_core::fonts::{FontCacheKey, FontError, FontSources, OptFontCache, normalize_font_ids};
use serde::Deserialize;
use tracing::{instrument, warn};

use crate::srv::server::{DebouncedWarning, map_error};

#[derive(Deserialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "unstable-schemas", into_params(parameter_in = Path))]
struct FontRequest {
    fontstack: String,
    start: u32,
    end: u32,
}

#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/font/{fontstack}/{start}-{end}",
        params(FontRequest),
        responses(
            (status = 200, description = "Glyph PBF range", content_type = "application/x-protobuf"),
            (status = 400, description = "Invalid glyph range"),
            (status = 404, description = "No matching font"),
        ),
    )
)]
#[route(
    "/font/{fontstack}/{start}-{end}",
    method = "GET",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
#[hotpath::measure]
#[instrument(
    level = "debug",
    skip_all,
    fields(
        font.fontstack = %path.fontstack,
        font.range.start = path.start,
        font.range.end = path.end,
    ),
    err(Debug),
)]
pub async fn get_font(
    path: Path<FontRequest>,
    fonts: Data<FontSources>,
    cache: Data<OptFontCache>,
) -> ActixResult<HttpResponse> {
    let result = if let Some(cache) = cache.as_ref() {
        // Key the cache by the fonts the request resolves to, not by the alias,
        // so invalidating a font also evicts entries reached through an alias.
        let expanded_ids = fonts.expand_font_ids(&path.fontstack);
        cache
            .get_or_insert(
                FontCacheKey::new(normalize_font_ids(&expanded_ids), path.start, path.end),
                async || fonts.get_font_range(&path.fontstack, path.start, path.end),
            )
            .await
    } else {
        fonts
            .get_font_range(&path.fontstack, path.start, path.end)
            .map_err(Arc::new)
    };
    let data = result.map_err(|e| map_font_error(e.as_ref()))?;
    Ok(HttpResponse::Ok()
        .content_type("application/x-protobuf")
        .body(data))
}

/// Redirect `/fonts/{fontstack}/{start}-{end}` to `/font/{fontstack}/{start}-{end}` (HTTP 301)
#[route("/fonts/{fontstack}/{start}-{end}", method = "GET", method = "HEAD")]
pub async fn redirect_fonts(path: Path<FontRequest>) -> HttpResponse {
    static WARNING: DebouncedWarning = DebouncedWarning::new();

    WARNING
        .once_per_hour(|| {
            warn!(
                "Request to /fonts/{}/{}-{} caused unnecessary redirect. Use /font/{}/{}-{} to avoid extra round-trip latency.",
                path.fontstack, path.start, path.end, path.fontstack, path.start, path.end
            );
        })
        .await;

    HttpResponse::MovedPermanently()
        .insert_header((
            LOCATION,
            format!("/font/{}/{}-{}", path.fontstack, path.start, path.end),
        ))
        .finish()
}

#[derive(Deserialize, Debug)]
struct FontExtRequest {
    fontstack: String,
    start: u32,
    end: u32,
    ext: String,
}

/// Redirect `/font/{fontstack}/{start}-{end}.{extension}` to `/font/{fontstack}/{start}-{end}` (HTTP 301)
#[routes]
#[get("/font/{fontstack}/{start}-{end}.{ext}")]
#[head("/font/{fontstack}/{start}-{end}.{ext}")]
#[get("/fonts/{fontstack}/{start}-{end}.{ext}")]
#[head("/fonts/{fontstack}/{start}-{end}.{ext}")]
pub async fn redirect_font_ext(path: Path<FontExtRequest>) -> HttpResponse {
    static WARNING: DebouncedWarning = DebouncedWarning::new();
    let FontExtRequest {
        fontstack,
        start,
        end,
        ext,
    } = path.as_ref();

    WARNING
        .once_per_hour(|| {
            warn!(
                "Request to /font/{fontstack}/{start}-{end}.{ext} caused unnecessary redirect. Use /font/{fontstack}/{start}-{end} to avoid extra round-trip latency."
            );
        })
        .await;

    HttpResponse::MovedPermanently()
        .insert_header((LOCATION, format!("/font/{fontstack}/{start}-{end}")))
        .finish()
}

pub fn map_font_error(e: &FontError) -> actix_web::Error {
    map_error(e)
}

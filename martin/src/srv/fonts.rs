use std::string::ToString;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use actix_middleware_etag::Etag;
use actix_web::error::{ErrorBadRequest, ErrorNotFound};
use actix_web::http::header::LOCATION;
use actix_web::middleware::Compress;
use actix_web::web::{Data, Path};
use actix_web::{HttpResponse, Result as ActixResult, route};
use martin_core::fonts::{FontError, FontSources, OptFontCache};
use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::warn;

use crate::srv::server::map_internal_error;

#[derive(Deserialize, Debug)]
struct FontRequest {
    fontstack: String,
    start: u32,
    end: u32,
}

#[route(
    "/font/{fontstack}/{start}-{end}",
    method = "GET",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
async fn get_font(
    path: Path<FontRequest>,
    fonts: Data<FontSources>,
    cache: Data<OptFontCache>,
) -> ActixResult<HttpResponse> {
    let result = if let Some(cache) = cache.as_ref() {
        cache
            .get_or_insert(path.fontstack.clone(), path.start, path.end, || {
                fonts.get_font_range(&path.fontstack, path.start, path.end)
            })
            .await
    } else {
        fonts.get_font_range(&path.fontstack, path.start, path.end)
    };
    let data = result.map_err(map_font_error)?;
    Ok(HttpResponse::Ok()
        .content_type("application/x-protobuf")
        .body(data))
}

/// Redirect `/fonts/{fontstack}/{start}-{end}` to `/font/{fontstack}/{start}-{end}` (HTTP 301)
#[route("/fonts/{fontstack}/{start}-{end}", method = "GET", method = "HEAD")]
pub async fn redirect_fonts(path: Path<FontRequest>) -> HttpResponse {
    static LAST_WARNING: LazyLock<Mutex<Instant>> = LazyLock::new(|| Mutex::new(Instant::now()));

    let mut warning = LAST_WARNING.lock().await;
    if warning.elapsed() >= Duration::from_hours(1) {
        *warning = Instant::now();
        warn!(
            "Using /fonts/{{fontstack}}/{{start}}-{{end}} endpoint which causes an unnecessary redirect. Use /font/{{fontstack}}/{{start}}-{{end}} directly to avoid extra round-trip latency."
        );
    }

    HttpResponse::MovedPermanently()
        .insert_header((
            LOCATION,
            format!("/font/{}/{}-{}", path.fontstack, path.start, path.end),
        ))
        .finish()
}

pub fn map_font_error(e: FontError) -> actix_web::Error {
    match e {
        FontError::FontNotFound(_) => ErrorNotFound(e.to_string()),
        FontError::InvalidFontRangeStartEnd(_, _)
        | FontError::InvalidFontRangeStart(_)
        | FontError::InvalidFontRangeEnd(_)
        | FontError::InvalidFontRange(_, _) => ErrorBadRequest(e.to_string()),
        _ => map_internal_error(e),
    }
}

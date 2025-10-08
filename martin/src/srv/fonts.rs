use std::string::ToString;

use actix_middleware_etag::Etag;
use actix_web::error::{ErrorBadRequest, ErrorNotFound};
use actix_web::middleware::Compress;
use actix_web::web::{Data, Path};
use actix_web::{HttpResponse, Result as ActixResult, route};
use martin_core::fonts::{FontError, FontSources, OptFontCache};
use serde::Deserialize;

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
#[allow(clippy::unused_async)]
async fn get_font(
    path: Path<FontRequest>,
    fonts: Data<FontSources>,
    cache: Data<OptFontCache>,
) -> ActixResult<HttpResponse> {
    let result = if let Some(cache) = cache.as_ref() {
        cache
            .get_or_insert(&path.fontstack, path.start, path.end, || {
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

pub fn map_font_error(e: FontError) -> actix_web::Error {
    #[allow(clippy::enum_glob_use)]
    use FontError::*;
    match e {
        FontNotFound(_) => ErrorNotFound(e.to_string()),
        InvalidFontRangeStartEnd(_, _)
        | InvalidFontRangeStart(_)
        | InvalidFontRangeEnd(_)
        | InvalidFontRange(_, _) => ErrorBadRequest(e.to_string()),
        _ => map_internal_error(e),
    }
}

use std::string::ToString;

use actix_middleware_etag::Etag;
use actix_web::error::{ErrorBadRequest, ErrorNotFound};
use actix_web::middleware::Compress;
use actix_web::web::{Data, Path};
use actix_web::{HttpResponse, Result as ActixResult, route};
use martin_core::fonts::{FontError, FontSources};
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
async fn get_font(path: Path<FontRequest>, fonts: Data<FontSources>) -> ActixResult<HttpResponse> {
    let result = fonts.get_font_range(&path.fontstack, path.start, path.end);
    let data = result.map_err(map_font_error)?;
    Ok(HttpResponse::Ok()
        .content_type("application/x-protobuf")
        .body(data))
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

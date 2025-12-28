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
    end: String,
    #[allow(dead_code)]
    ext: String,
}

impl FontRequest {
    fn get_end_value(&self) -> Result<u32, std::num::ParseIntError> {
        self.end.parse()
    }
}

#[route(
    "/font/{fontstack}/{start}-{end:[0-9]+}{ext:(\\.pbf)?}",
    method = "GET",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
pub async fn get_font(
    path: Path<FontRequest>,
    fonts: Data<FontSources>,
    cache: Data<OptFontCache>,
) -> ActixResult<HttpResponse> {
    let end = path.get_end_value().map_err(|e| {
        ErrorBadRequest(format!("Invalid end parameter: {e}"))
    })?;
    
    let result = if let Some(cache) = cache.as_ref() {
        cache
            .get_or_insert(path.fontstack.clone(), path.start, end, || {
                fonts.get_font_range(&path.fontstack, path.start, end)
            })
            .await
    } else {
        fonts.get_font_range(&path.fontstack, path.start, end)
    };
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

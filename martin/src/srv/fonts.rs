use std::string::ToString;

use actix_web::error::{ErrorBadRequest, ErrorNotFound};
use actix_web::web::{Data, Path};
use actix_web::{middleware, route, HttpResponse, Result as ActixResult};
use serde::Deserialize;

use crate::fonts::{FontError, FontSources};
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
    wrap = "middleware::Compress::default()"
)]
#[allow(clippy::unused_async)]
async fn get_font(path: Path<FontRequest>, fonts: Data<FontSources>) -> ActixResult<HttpResponse> {
    let data = fonts
        .get_font_range(&path.fontstack, path.start, path.end)
        .map_err(map_font_error)?;
    Ok(HttpResponse::Ok()
        .content_type("application/x-protobuf")
        .body(data))
}

/// Returns the available font stacks
///
/// Separate from the [`/catalog`](crate::srv::server::get_catalog) route to allow editors like
/// [maputnik](https://maputnik.github.io/) to list fonts.
/// To match the route `/font/%7Bfontstack%7D/%7Brange%7D`, macros are not possible due the route
/// clashing with the path-parameter mechanism.
#[allow(clippy::unused_async)]
pub(crate) async fn get_fontstacks(fonts: Data<FontSources>) -> HttpResponse {
    let fontstacks = fonts.get_fontstacks();
    HttpResponse::Ok().json(fontstacks)
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

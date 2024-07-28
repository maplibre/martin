use std::str::FromStr;
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
    start: String,
    end: String,
}

impl FontRequest {
    fn parse(&self) -> Result<(u32, u32), &'static str> {
        let start = u32::from_str(&self.start).map_err(|_| "Invalid start value")?;
        let end = FontRequest::parse_leading_digits(&self.end)?;

        Ok((start, end))
    }

    fn parse_leading_digits(input: &str) -> Result<u32, &'static str> {
        let digits: String = input.chars().take_while(|c| c.is_digit(10)).collect();
        if digits.is_empty() {
            Err("No leading digits found")
        } else {
            digits.parse::<u32>().map_err(|_| "Failed to parse number")
        }
    }
}

#[route(
    "/font/{fontstack}/{start}-{end}*",
    method = "GET",
    wrap = "middleware::Compress::default()"
)]
#[allow(clippy::unused_async)]
async fn get_font(path: Path<FontRequest>, fonts: Data<FontSources>) -> ActixResult<HttpResponse> {
    let (start, end) = path.parse().map_err(|e| ErrorBadRequest(e.to_string()))?;

    let data = fonts
        .get_font_range(&path.fontstack, start, end)
        .map_err(map_font_error)?;
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

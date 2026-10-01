#![cfg_attr(
    not(target_os = "linux"),
    expect(
        dead_code,
        reason = "the routes are registered on Linux only, the module is compiled so utoipa can describe them"
    )
)]

use std::io::Cursor;
use std::num::{IntErrorKind, NonZeroU8};

use actix_web::http::header::{ContentType, LOCATION};
use actix_web::web::{Data, Path};
use actix_web::{HttpResponse, route};
use image::{DynamicImage, ImageFormat};
use martin_core::styles::StyleSources;
use martin_tile_utils::TileCoord;
use serde::Deserialize;
use tracing::{error, trace, warn};

use crate::srv::server::DebouncedWarning;

/// Image format requested in the URL.
#[derive(Deserialize, Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub(super) enum ImageFormatRequest {
    /// `image/png` - lossless, supports alpha.
    #[default]
    Png,
    /// `image/jpeg` - lossy, no alpha (RGBA is flattened to RGB on encode).
    #[serde(rename = "jpg", alias = "jpeg")]
    Jpeg,
    /// `image/webp` - lossless WebP via the `image` crate.
    Webp,
}

impl ImageFormatRequest {
    const fn image_format(self) -> ImageFormat {
        match self {
            Self::Png => ImageFormat::Png,
            Self::Jpeg => ImageFormat::Jpeg,
            Self::Webp => ImageFormat::WebP,
        }
    }

    fn content_type(self) -> ContentType {
        match self {
            Self::Png => ContentType::png(),
            Self::Jpeg => ContentType::jpeg(),
            Self::Webp => ContentType("image/webp".parse().expect("static MIME parses")),
        }
    }
}

/// Encode `img` into `format` and wrap it in a successful [`HttpResponse`].
/// JPEG has no alpha channel, so RGBA is flattened to RGB before encoding.
pub(super) fn encode_image_response(
    img: &image::RgbaImage,
    format: ImageFormatRequest,
) -> HttpResponse {
    let image_format = format.image_format();
    let dynamic_img = DynamicImage::ImageRgba8(img.clone());
    let to_encode = if image_format == ImageFormat::Jpeg {
        DynamicImage::ImageRgb8(dynamic_img.to_rgb8())
    } else {
        dynamic_img
    };

    let mut output = Cursor::new(Vec::new());
    match to_encode.write_to(&mut output, image_format) {
        Ok(()) => HttpResponse::Ok()
            .content_type(format.content_type())
            .body(output.into_inner()),
        Err(e) => {
            error!("Failed to encode image: {e}");
            HttpResponse::InternalServerError()
                .content_type(ContentType::plaintext())
                .body("Failed to encode image")
        }
    }
}

#[derive(Deserialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "unstable-schemas", into_params(parameter_in = Path))]
struct StyleRenderRequest {
    style_id: String,
    z: u8,
    x: u32,
    /// The row, optionally followed by `@{n}x` for a tile drawn at pixel ratio `n` (`n` times the pixels).
    y: String,
    #[cfg_attr(feature = "unstable-schemas", param(inline))]
    format: ImageFormatRequest,
}

/// Why a `{y}` path segment was rejected.
#[derive(Debug, PartialEq, Eq)]
enum RowError {
    Row,
    PixelRatio,
    AbovePixelRatio(NonZeroU8),
}

impl RowError {
    fn response(&self) -> HttpResponse {
        let body = match self {
            Self::Row => "Invalid tile row".to_owned(),
            Self::PixelRatio => {
                "Invalid pixel ratio, expected {y}@{n}x with a whole number n of at least 1"
                    .to_owned()
            }
            Self::AbovePixelRatio(max) => format!("Pixel ratio above @{max}x is not served"),
        };
        HttpResponse::BadRequest()
            .content_type(ContentType::plaintext())
            .body(body)
    }
}

/// Splits `123` / `123@3x` into the row and the requested pixel ratio (1 when absent),
/// accepting ratios from 1 to `max`.
fn parse_row(y: &str, max: NonZeroU8) -> Result<(u32, NonZeroU8), RowError> {
    let (row, pixel_ratio) = match y.split_once('@') {
        None => (y, NonZeroU8::MIN),
        Some((row, suffix)) => (row, parse_pixel_ratio(suffix, max)?),
    };
    let row = row.parse().ok().ok_or(RowError::Row)?;
    Ok((row, pixel_ratio))
}

/// Parses the `{n}x` of an `@{n}x` suffix: `n` is plain decimal digits without leading zeros.
fn parse_pixel_ratio(suffix: &str, max: NonZeroU8) -> Result<NonZeroU8, RowError> {
    let digits = suffix.strip_suffix('x').ok_or(RowError::PixelRatio)?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(RowError::PixelRatio);
    }
    match digits.parse::<NonZeroU8>() {
        Ok(ratio) if ratio <= max => Ok(ratio),
        Ok(_) => Err(RowError::AbovePixelRatio(max)),
        Err(e) if *e.kind() == IntErrorKind::PosOverflow => Err(RowError::AbovePixelRatio(max)),
        Err(_) => Err(RowError::PixelRatio),
    }
}

#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/style/{style_id}/{z}/{x}/{y}.{format}",
        params(StyleRenderRequest),
        responses(
            (status = 200, description = "Server-side rendered style tile (PNG/JPEG/WebP)"),
            (status = 400, description = "Invalid tile coordinates, or a pixel ratio that is malformed or above `max_pixel_ratio`"),
            (status = 403, description = "Rendering is disabled"),
            (status = 404, description = "No matching style"),
            (status = 500, description = "Renderer or encoder failure"),
        ),
    )
)]
#[route("/style/{style_id}/{z}/{x}/{y}.{format}", method = "GET")]
#[hotpath::measure]
pub async fn get_rendered_tile_style(
    path: Path<StyleRenderRequest>,
    styles: Data<StyleSources>,
) -> HttpResponse {
    let style_id = &path.style_id;
    let Some(style_path) = styles.style_json_path(style_id) else {
        return HttpResponse::NotFound()
            .content_type(ContentType::plaintext())
            .body("No such style exists");
    };
    let (y, pixel_ratio) = match parse_row(&path.y, styles.max_pixel_ratio()) {
        Ok(parsed) => parsed,
        Err(e) => return e.response(),
    };
    let Some(zxy) = TileCoord::new_checked(path.z, path.x, y) else {
        return HttpResponse::BadRequest()
            .content_type(ContentType::plaintext())
            .body("Invalid tile coordinates for zoom level");
    };
    trace!(
        "Rendering style {style_id} ({}) at {zxy}@{pixel_ratio}x",
        style_path.display()
    );

    #[cfg(target_os = "linux")]
    let response = {
        use martin_core::styles::StyleError;

        match styles
            .render_with_pixel_ratio(style_path, zxy.z(), zxy.x(), zxy.y(), pixel_ratio)
            .await
        {
            Ok(image) => encode_image_response(image.as_image(), path.format),
            Err(StyleError::RenderingIsDisabled) => rendering_disabled(style_id, zxy),
            Err(e) => {
                error!("Failed to render style {style_id} at {zxy}: {e}");
                HttpResponse::InternalServerError()
                    .content_type(ContentType::plaintext())
                    .body("Failed to render style")
            }
        }
    };
    #[cfg(not(target_os = "linux"))]
    let response = rendering_disabled(style_id, zxy);
    response
}

fn rendering_disabled(style_id: &str, zxy: TileCoord) -> HttpResponse {
    warn!("Failed to render style {style_id} because rendering is disabled");
    HttpResponse::Forbidden()
        .content_type(ContentType::plaintext())
        .body(format!(
            "Failed to render style {style_id} at {zxy} is forbidden as rendering is disabled"
        ))
}

/// `.jpeg` to `.jpg` redirect
#[derive(Deserialize, Debug)]
struct TileJpegRedirectPath {
    style_id: String,
    z: u8,
    x: u32,
    /// The row, optionally followed by `@{n}x`, kept as is in the redirect.
    y: String,
}

/// Redirect `/style/{id}/{z}/{x}/{y}.jpeg` (and `{y}@{n}x.jpeg`) to the canonical `.jpg` form
/// (HTTP 301). Same pattern as the static endpoint's `.jpeg` redirect.
#[route("/style/{style_id}/{z}/{x}/{y}.jpeg", method = "GET", method = "HEAD")]
pub async fn redirect_tile_jpeg(
    path: Path<TileJpegRedirectPath>,
    styles: Data<StyleSources>,
) -> HttpResponse {
    static WARNING: DebouncedWarning = DebouncedWarning::new();
    let TileJpegRedirectPath { style_id, z, x, y } = path.as_ref();
    if let Err(e) = parse_row(y, styles.max_pixel_ratio()) {
        return e.response();
    }
    WARNING
        .once_per_hour(|| {
            warn!(
                "Request to /style/{style_id}/{z}/{x}/{y}.jpeg caused unnecessary redirect. Use .jpg to avoid extra round-trip latency."
            );
        })
        .await;
    HttpResponse::MovedPermanently()
        .insert_header((LOCATION, format!("/style/{style_id}/{z}/{x}/{y}.jpg")))
        .finish()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const MAX: NonZeroU8 = NonZeroU8::new(4).expect("4 is non-zero");

    fn ratio(n: u8) -> NonZeroU8 {
        NonZeroU8::new(n).expect("non-zero")
    }

    #[rstest]
    #[case::no_suffix("5", 5, 1)]
    #[case::one_x("5@1x", 5, 1)]
    #[case::two_x("5@2x", 5, 2)]
    #[case::the_max("5@4x", 5, 4)]
    #[case::a_large_row("123456@3x", 123_456, 3)]
    fn a_row_parses(#[case] y: &str, #[case] row: u32, #[case] pixel_ratio: u8) {
        assert_eq!(parse_row(y, MAX), Ok((row, ratio(pixel_ratio))));
    }

    #[rstest]
    #[case::not_a_number("a@2x")]
    #[case::no_row("@2x")]
    #[case::negative("-5")]
    #[case::past_u32("4294967296")]
    fn a_bad_row_is_rejected(#[case] y: &str) {
        assert_eq!(parse_row(y, MAX), Err(RowError::Row));
    }

    #[rstest]
    #[case::zero("5@0x")]
    #[case::no_number("5@x")]
    #[case::fractional("5@1.5x")]
    #[case::no_x("5@2")]
    #[case::upper_case_x("5@2X")]
    #[case::a_plus_sign("5@+2x")]
    #[case::a_leading_zero("5@02x")]
    #[case::twice("5@2x@2x")]
    #[case::empty("5@")]
    #[case::whitespace("5@ 2x")]
    fn a_bad_pixel_ratio_is_rejected(#[case] y: &str) {
        assert_eq!(parse_row(y, MAX), Err(RowError::PixelRatio));
    }

    #[rstest]
    #[case::just_above("5@5x")]
    #[case::past_u8("5@256x")]
    #[case::far_past_u8("5@99999999999x")]
    fn a_pixel_ratio_above_the_max_is_rejected(#[case] y: &str) {
        assert_eq!(parse_row(y, MAX), Err(RowError::AbovePixelRatio(MAX)));
    }

    #[test]
    fn a_lower_max_caps_the_pixel_ratio() {
        assert_eq!(parse_row("5@2x", ratio(2)), Ok((5, ratio(2))));
        assert_eq!(
            parse_row("5@3x", ratio(2)),
            Err(RowError::AbovePixelRatio(ratio(2)))
        );
    }
}

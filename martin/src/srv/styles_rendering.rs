#![cfg_attr(
    not(target_os = "linux"),
    expect(
        dead_code,
        reason = "the routes are registered on Linux only, the module is compiled so utoipa can describe them"
    )
)]

mod png_palette;

use std::io::Cursor;

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

#[derive(thiserror::Error, Debug)]
enum EncodeError {
    #[error(transparent)]
    Image(#[from] image::ImageError),
    #[error(transparent)]
    Palette(#[from] png_palette::PngPaletteError),
}

/// Encode `img` into `format` and wrap it in a successful [`HttpResponse`].
/// A PNG is indexed with at most `png_max_colors` colours if set, and RGBA otherwise.
pub(super) fn encode_image_response(
    img: &image::RgbaImage,
    format: ImageFormatRequest,
    png_max_colors: Option<u16>,
) -> HttpResponse {
    match encode_image(img, format, png_max_colors) {
        Ok(bytes) => HttpResponse::Ok()
            .content_type(format.content_type())
            .body(bytes),
        Err(e) => {
            error!("Failed to encode image: {e}");
            HttpResponse::InternalServerError()
                .content_type(ContentType::plaintext())
                .body("Failed to encode image")
        }
    }
}

/// JPEG has no alpha channel, so RGBA is flattened to RGB before encoding.
fn encode_image(
    img: &image::RgbaImage,
    format: ImageFormatRequest,
    png_max_colors: Option<u16>,
) -> Result<Vec<u8>, EncodeError> {
    if let (ImageFormatRequest::Png, Some(max_colors)) = (format, png_max_colors) {
        return Ok(png_palette::encode(img, max_colors)?);
    }
    let image_format = format.image_format();
    let dynamic_img = DynamicImage::ImageRgba8(img.clone());
    let to_encode = if image_format == ImageFormat::Jpeg {
        DynamicImage::ImageRgb8(dynamic_img.to_rgb8())
    } else {
        dynamic_img
    };

    let mut output = Cursor::new(Vec::new());
    to_encode.write_to(&mut output, image_format)?;
    Ok(output.into_inner())
}

#[derive(Deserialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "unstable-schemas", into_params(parameter_in = Path))]
struct StyleRenderRequest {
    style_id: String,
    z: u8,
    x: u32,
    y: u32,
    #[cfg_attr(feature = "unstable-schemas", param(inline))]
    format: ImageFormatRequest,
}

#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/style/{style_id}/{z}/{x}/{y}.{format}",
        params(StyleRenderRequest),
        responses(
            (status = 200, description = "Server-side rendered style tile (PNG/JPEG/WebP)"),
            (status = 400, description = "Invalid tile coordinates"),
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
    let Some(zxy) = TileCoord::new_checked(path.z, path.x, path.y) else {
        return HttpResponse::BadRequest()
            .content_type(ContentType::plaintext())
            .body("Invalid tile coordinates for zoom level");
    };
    trace!(
        "Rendering style {style_id} ({}) at {zxy}",
        style_path.display()
    );

    #[cfg(target_os = "linux")]
    let response = {
        use martin_core::styles::StyleError;

        match styles.render(style_path, zxy.z(), zxy.x(), zxy.y()).await {
            Ok(image) => {
                encode_image_response(image.as_image(), path.format, styles.png_max_colors())
            }
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
    y: u32,
}

/// Redirect `/style/{id}/{z}/{x}/{y}.jpeg` to the canonical `.jpg` form
/// (HTTP 301). Same pattern as the static endpoint's `.jpeg` redirect.
#[route("/style/{style_id}/{z}/{x}/{y}.jpeg", method = "GET", method = "HEAD")]
pub async fn redirect_tile_jpeg(path: Path<TileJpegRedirectPath>) -> HttpResponse {
    static WARNING: DebouncedWarning = DebouncedWarning::new();
    let TileJpegRedirectPath { style_id, z, x, y } = path.as_ref();
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

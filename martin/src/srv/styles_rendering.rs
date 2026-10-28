#![cfg_attr(
    not(target_os = "linux"),
    expect(
        dead_code,
        reason = "the routes are registered on Linux only, the module is compiled so utoipa can describe them"
    )
)]

mod png_palette;

use std::io::Cursor;
#[cfg(target_os = "linux")]
use std::num::NonZero;
use std::num::NonZeroU8;
#[cfg(target_os = "linux")]
use std::sync::LazyLock;

use actix_web::http::header::{ContentType, LOCATION};
use actix_web::web::{Data, Path};
use actix_web::{HttpResponse, routes};
use image::codecs::jpeg::JpegEncoder;
use image::{ImageFormat, RgbaImage};
use martin_core::styles::{ImageQualities, ImageQuality, StyleSources};
use martin_tile_utils::TileCoord;
use serde::Deserialize;
use tokio::sync::Semaphore;
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
    /// `image/jpeg` - lossy at the configured quality, no alpha (dropped on encode).
    #[serde(rename = "jpg", alias = "jpeg")]
    Jpeg,
    /// `image/webp` - lossy WebP via libwebp at the configured quality, or lossless. Supports alpha.
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

/// Bounds concurrent encodes across the process, given that this is CPU bound.
#[cfg(target_os = "linux")]
static ENCODE_PERMITS: LazyLock<Semaphore> = LazyLock::new(|| {
    let cores = std::thread::available_parallelism().map_or(4, NonZero::get);
    Semaphore::new(cores)
});

#[derive(thiserror::Error, Debug)]
enum EncodeError {
    #[error("the server is shutting down")]
    ShuttingDown,
    #[error("the encode task did not complete: {0}")]
    Task(#[from] tokio::task::JoinError),
    #[error(transparent)]
    Image(#[from] image::ImageError),
    #[error(transparent)]
    Palette(#[from] png_palette::PngPaletteError),
    #[error("Failed to encode a lossy WebP: {0:?}")]
    Webp(webp::WebPEncodingError),
    #[cfg(feature = "ogcapi")]
    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

/// Encode `image` into `format` and wrap it in a successful [`HttpResponse`].
/// A PNG is indexed with at most `png_max_colors` colours if set, and RGBA otherwise.
/// A WebP and a JPEG are encoded at their `quality`.
#[cfg(target_os = "linux")]
pub(super) async fn encode_image_response(
    image: martin_core::styles::StaticImage,
    format: ImageFormatRequest,
    png_max_colors: Option<u16>,
    quality: ImageQualities,
) -> HttpResponse {
    encode_scaled_image_response(image, None, format, png_max_colors, quality).await
}

/// Like [`encode_image_response`], scaling `image` down to `size` first when given.
#[cfg(target_os = "linux")]
pub(super) async fn encode_scaled_image_response(
    image: martin_core::styles::StaticImage,
    size: Option<(u32, u32)>,
    format: ImageFormatRequest,
    png_max_colors: Option<u16>,
    quality: ImageQualities,
) -> HttpResponse {
    match encode_off_worker(&ENCODE_PERMITS, move || match size {
        Some((width, height)) => {
            let scaled =
                image::imageops::resize(image.as_image(), width, height, image::imageops::FilterType::Triangle);
            encode_image(&scaled, format, png_max_colors, quality)
        }
        None => encode_image(image.as_image(), format, png_max_colors, quality),
    })
    .await
    {
        Ok(bytes) => HttpResponse::Ok().content_type(format.content_type()).body(bytes),
        Err(e) => {
            error!(error = %e, "Failed to encode image");
            HttpResponse::InternalServerError()
                .content_type(ContentType::plaintext())
                .body("Failed to encode image")
        }
    }
}

/// Run `encode` under the limit image encodes share, and answer its bytes as `content_type`.
#[cfg(all(feature = "ogcapi", target_os = "linux"))]
pub(super) async fn encode_response<E: std::error::Error + Send + Sync + 'static>(
    content_type: &'static str,
    encode: impl FnOnce() -> Result<Vec<u8>, E> + Send + 'static,
) -> HttpResponse {
    match encode_off_worker(&ENCODE_PERMITS, move || {
        encode().map_err(|e| EncodeError::Other(Box::new(e)))
    })
    .await
    {
        Ok(bytes) => HttpResponse::Ok().content_type(content_type).body(bytes),
        Err(e) => {
            error!(error = %e, "Failed to encode image");
            HttpResponse::InternalServerError()
                .content_type(ContentType::plaintext())
                .body("Failed to encode image")
        }
    }
}

/// Run `encode` on the blocking pool while holding one of `permits`.
async fn encode_off_worker(
    permits: &'static Semaphore,
    encode: impl FnOnce() -> Result<Vec<u8>, EncodeError> + Send + 'static,
) -> Result<Vec<u8>, EncodeError> {
    let permit = permits
        .acquire()
        .await
        // The only way to fail is a closed semaphore, which happens at shutdown.
        .map_err(|_closed| EncodeError::ShuttingDown)?;
    // Encoding is CPU-bound for milliseconds, which would stall
    // every other request on this actix worker if it ran inline.
    tokio::task::spawn_blocking(move || {
        // The permit lives in the task, not in the request future: a cancelled
        // request must not free it while the encode is still running.
        let _permit = permit;
        encode()
    })
    .await?
}

/// Encode `img` into `format`, as an indexed PNG with at most `png_max_colors` colours if set,
/// and as a WebP or JPEG at its `quality`.
/// JPEG has no alpha channel, so the alpha is dropped.
fn encode_image(
    img: &RgbaImage,
    format: ImageFormatRequest,
    png_max_colors: Option<u16>,
    quality: ImageQualities,
) -> Result<Vec<u8>, EncodeError> {
    let mut output = Cursor::new(Vec::new());
    match (format, png_max_colors, quality.webp) {
        (ImageFormatRequest::Png, Some(max_colors), _) => return Ok(png_palette::encode(img, max_colors)?),
        (ImageFormatRequest::Webp, _, ImageQuality::Lossy(quality)) => {
            // libwebp keeps the alpha channel lossless next to the lossy colours.
            let webp = webp::Encoder::from_rgba(img.as_raw(), img.width(), img.height())
                .encode_simple(false, f32::from(quality.get()))
                .map_err(EncodeError::Webp)?;
            return Ok(webp.to_vec());
        }
        (ImageFormatRequest::Jpeg, _, _) => {
            // Drops the alpha channel while it encodes, without an RGB copy of the image.
            JpegEncoder::new_with_quality(&mut output, quality.jpeg.get()).encode_image(img)?;
        }
        // An RGBA PNG or a lossless WebP.
        _ => img.write_to(&mut output, format.image_format())?,
    }
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
    /// The `n` of a `{y}@{n}x` request: the tile is drawn at `n` times the pixels. Absent means 1.
    #[cfg_attr(feature = "unstable-schemas", param(ignore))]
    pixel_ratio: Option<NonZeroU8>,
    #[cfg_attr(feature = "unstable-schemas", param(inline))]
    format: ImageFormatRequest,
}

#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/style/{style_id}/{z}/{x}/{y}.{format}",
        description = "Append `@{n}x` to `{y}` (`/style/{style_id}/{z}/{x}/{y}@2x.png`) to draw the tile at `n` times the pixels.",
        params(StyleRenderRequest),
        responses(
            (status = 200, description = "Server-side rendered style tile (PNG/JPEG/WebP)"),
            (status = 400, description = "Invalid tile coordinates, or a pixel ratio not listed in `pixel_ratios`"),
            (status = 403, description = "Rendering is disabled"),
            (status = 404, description = "No matching style"),
            (status = 500, description = "Renderer or encoder failure"),
        ),
    )
)]
#[routes]
#[get("/style/{style_id}/{z}/{x}/{y:[0-9]+}@{pixel_ratio:[1-9][0-9]*}x.{format}")]
#[get("/style/{style_id}/{z}/{x}/{y:[0-9]+}.{format}")]
#[hotpath::measure]
pub async fn get_rendered_tile_style(path: Path<StyleRenderRequest>, styles: Data<StyleSources>) -> HttpResponse {
    let style_id = &path.style_id;
    let Some(style_path) = styles.style_json_path(style_id) else {
        return HttpResponse::NotFound()
            .content_type(ContentType::plaintext())
            .body("No such style exists");
    };
    let pixel_ratio = path.pixel_ratio.unwrap_or(NonZeroU8::MIN);
    if let Err(err) = styles.pixel_ratios().check(pixel_ratio) {
        return HttpResponse::BadRequest()
            .content_type(ContentType::plaintext())
            .body(err.to_string());
    }
    let Some(zxy) = TileCoord::new_checked(path.z, path.x, path.y) else {
        return HttpResponse::BadRequest()
            .content_type(ContentType::plaintext())
            .body("Invalid tile coordinates for zoom level");
    };
    trace!(
        style.id = %style_id,
        style.path = %style_path.display(),
        tile.coord = %zxy,
        style.pixel_ratio = %pixel_ratio,
        "Rendering style"
    );

    #[cfg(target_os = "linux")]
    let response = {
        use martin_core::styles::StyleError;

        match styles
            .render_with_pixel_ratio(style_path, zxy.z(), zxy.x(), zxy.y(), pixel_ratio)
            .await
        {
            Ok(image) => {
                encode_image_response(image, path.format, styles.png_max_colors(), styles.image_quality()).await
            }
            Err(StyleError::RenderingIsDisabled) => rendering_disabled(style_id, zxy),
            Err(e) => {
                error!(style.id = %style_id, tile.coord = %zxy, error = %e, "Failed to render style");
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
    warn!(style.id = %style_id, "Failed to render style because rendering is disabled");
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
    pixel_ratio: Option<NonZeroU8>,
}

/// Redirect `/style/{id}/{z}/{x}/{y}.jpeg` (and `{y}@{n}x.jpeg`) to the canonical `.jpg` form
/// (HTTP 301). Same pattern as the static endpoint's `.jpeg` redirect.
#[routes]
#[get("/style/{style_id}/{z}/{x}/{y:[0-9]+}@{pixel_ratio:[1-9][0-9]*}x.jpeg")]
#[head("/style/{style_id}/{z}/{x}/{y:[0-9]+}@{pixel_ratio:[1-9][0-9]*}x.jpeg")]
#[get("/style/{style_id}/{z}/{x}/{y:[0-9]+}.jpeg")]
#[head("/style/{style_id}/{z}/{x}/{y:[0-9]+}.jpeg")]
pub async fn redirect_tile_jpeg(path: Path<TileJpegRedirectPath>) -> HttpResponse {
    static WARNING: DebouncedWarning = DebouncedWarning::new();
    let TileJpegRedirectPath {
        style_id,
        z,
        x,
        y,
        pixel_ratio,
    } = path.as_ref();
    let tile = match pixel_ratio {
        Some(n) => format!("/style/{style_id}/{z}/{x}/{y}@{n}x"),
        None => format!("/style/{style_id}/{z}/{x}/{y}"),
    };
    WARNING
        .once_per_hour(|| {
            warn!(
                style.tile = %tile,
                "Request to a .jpeg style tile caused unnecessary redirect. Use .jpg to avoid extra round-trip latency."
            );
        })
        .await;
    HttpResponse::MovedPermanently()
        .insert_header((LOCATION, format!("{tile}.jpg")))
        .finish()
}

#[cfg(test)]
mod tests {
    use image::ColorType;
    use rstest::rstest;
    use tokio::sync::oneshot;

    use martin_core::styles::LossyQuality;

    use super::*;

    fn lossy(quality: u8) -> LossyQuality {
        LossyQuality::new(quality).expect("a quality from 1 to 100")
    }

    fn webp_at(webp: ImageQuality) -> ImageQualities {
        ImageQualities {
            webp,
            ..ImageQualities::default()
        }
    }

    fn translucent_image() -> RgbaImage {
        let bytes = (0..=u8::MAX).cycle().take(64 * 32 * 4).collect();
        RgbaImage::from_raw(64, 32, bytes).expect("the buffer fits 64x32 RGBA")
    }

    #[rstest]
    #[case::png(ImageFormatRequest::Png)]
    #[case::webp(ImageFormatRequest::Webp)]
    fn a_lossless_encode_keeps_every_pixel(#[case] format: ImageFormatRequest) {
        let image = translucent_image();

        let encoded = encode_image(&image, format, None, webp_at(ImageQuality::Lossless)).expect("encodes");

        let decoded = image::load_from_memory_with_format(&encoded, format.image_format()).expect("decodes");
        assert_eq!(decoded.to_rgba8(), image);
    }

    #[tokio::test]
    async fn a_cancelled_request_keeps_the_permit_until_the_encode_returns() {
        static PERMITS: Semaphore = Semaphore::const_new(1);
        let (started_tx, started_rx) = oneshot::channel();
        let (finish_tx, finish_rx) = oneshot::channel();
        let request = tokio::spawn(encode_off_worker(&PERMITS, move || {
            started_tx.send(()).expect("the test waits for the start");
            finish_rx.blocking_recv().expect("the test releases the encode");
            Ok(Vec::new())
        }));
        started_rx.await.expect("the encode starts");

        request.abort();
        assert!(request.await.expect_err("the request is cancelled").is_cancelled());
        assert_eq!(
            PERMITS.available_permits(),
            0,
            "the running encode still holds the permit"
        );

        finish_tx.send(()).expect("the encode waits for the release");
        let _permit = PERMITS
            .acquire()
            .await
            .expect("the permit is released once the encode returns");
    }

    #[test]
    fn a_jpeg_encode_drops_the_alpha_channel() {
        let encoded = encode_image(
            &translucent_image(),
            ImageFormatRequest::Jpeg,
            None,
            ImageQualities::default(),
        )
        .expect("encodes");

        let decoded = image::load_from_memory_with_format(&encoded, ImageFormat::Jpeg).expect("decodes");
        assert_eq!(decoded.color(), ColorType::Rgb8);
        assert_eq!((decoded.width(), decoded.height()), (64, 32));
    }

    /// A 512×512 tile rendered by `MapLibre` Native from the `maptiler_basic` style.
    fn rendered_tile() -> RgbaImage {
        image::open("../tests/fixtures/rendering_references/maptiler_basic_0_0_0.png")
            .expect("the rendered reference tile loads")
            .to_rgba8()
    }

    fn webp(img: &RgbaImage, quality: ImageQuality) -> Vec<u8> {
        encode_image(img, ImageFormatRequest::Webp, None, webp_at(quality)).expect("encodes")
    }

    fn decode_webp(bytes: &[u8]) -> RgbaImage {
        image::load_from_memory_with_format(bytes, ImageFormat::WebP)
            .expect("decodes")
            .to_rgba8()
    }

    #[test]
    fn a_lossy_webp_of_a_rendered_tile_is_a_fraction_of_lossless() {
        let tile = rendered_tile();

        let lossless = webp(&tile, ImageQuality::Lossless);
        let lossy = webp(&tile, ImageQuality::default());

        assert!(
            lossy.len() * 3 < lossless.len(),
            "lossy {} bytes, lossless {} bytes",
            lossy.len(),
            lossless.len()
        );
        assert_eq!(decode_webp(&lossy).dimensions(), tile.dimensions());
    }

    #[test]
    fn a_lossy_webp_keeps_the_alpha_channel() {
        let image = translucent_image();

        let decoded = decode_webp(&webp(&image, ImageQuality::default()));

        let alpha = |img: &RgbaImage| img.pixels().map(|px| px.0[3]).collect::<Vec<_>>();
        assert_eq!(alpha(&decoded), alpha(&image));
    }

    #[test]
    fn an_empty_image_fails_to_encode_as_a_lossy_webp() {
        let err = encode_image(
            &RgbaImage::new(0, 0),
            ImageFormatRequest::Webp,
            None,
            ImageQualities::default(),
        )
        .expect_err("nothing to encode");
        assert!(matches!(err, EncodeError::Webp(_)), "{err}");
    }

    #[test]
    fn a_jpeg_follows_its_own_quality() {
        let tile = rendered_tile();
        let jpeg = |webp, jpeg| {
            encode_image(&tile, ImageFormatRequest::Jpeg, None, ImageQualities { webp, jpeg })
                .expect("encodes")
                .len()
        };

        assert!(jpeg(ImageQuality::default(), lossy(10)) < jpeg(ImageQuality::default(), lossy(80)));
        assert!(jpeg(ImageQuality::default(), lossy(80)) < jpeg(ImageQuality::default(), lossy(100)));
        assert_eq!(
            jpeg(ImageQuality::Lossless, lossy(80)),
            jpeg(ImageQuality::Lossy(lossy(10)), lossy(80)),
            "the WebP quality does not change a JPEG"
        );
    }
}

use actix_web::http::header::ContentType;
use actix_web::web::{Data, Path};
use actix_web::{HttpResponse, route};
use log::{error, trace, warn};
use martin_core::styles::StyleSources;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct StyleRenderRequest {
    style_id: String,
    z: u8,
    x: u32,
    y: u32,
    format: ImageFormatRequest,
}

#[derive(Deserialize, Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
enum ImageFormatRequest {
    #[default]
    Png,
}

#[route("/style/{style_id}/{z}/{x}/{y}.{format}", method = "GET")]
pub async fn get_style_rendered(
    path: Path<StyleRenderRequest>,
    styles: Data<StyleSources>,
) -> HttpResponse {
    use martin_core::styles::StyleError;

    let style_id = &path.style_id;
    let Some(style_path) = styles.style_json_path(style_id) else {
        return HttpResponse::NotFound()
            .content_type(ContentType::plaintext())
            .body("No such style exists");
    };
    let Some(zxy) = martin_tile_utils::TileCoord::new_checked(path.z, path.x, path.y) else {
        return HttpResponse::BadRequest()
            .content_type(ContentType::plaintext())
            .body("Invalid tile coordinates for zoom level");
    };
    trace!(
        "Rendering style {style_id} ({}) at {zxy}",
        style_path.display()
    );

    let image = styles.render(style_path, zxy.z, zxy.x, zxy.y).await;
    let image = match image {
        Ok(image) => image,
        Err(StyleError::RenderingIsDisabled) => {
            warn!("Failed to render style {style_id} because rendering is disabled");
            return HttpResponse::Forbidden()
                .content_type(ContentType::plaintext())
                .body(format!("Failed to render style {style_id} at {zxy} is forbidden as rendering is disabled"));
        }
        Err(e) => {
            error!("Failed to render style {style_id} at {zxy}: {e}");
            return HttpResponse::InternalServerError()
                .content_type(ContentType::plaintext())
                .body("Failed to render style");
        }
    };

    // Re-encode to target format
    let mut img_buffer = std::io::Cursor::new(Vec::new());
    let (image_format, content_type) = match path.format {
        ImageFormatRequest::Png => (image::ImageFormat::Png, ContentType::png()),
    };

    let image_encoding_result = image.as_image().write_to(&mut img_buffer, image_format);
    match image_encoding_result {
        Ok(()) => HttpResponse::Ok()
            .content_type(content_type)
            .body(img_buffer.into_inner()),
        Err(e) => {
            error!("Failed to encode image: {e}");
            HttpResponse::InternalServerError()
                .content_type(ContentType::plaintext())
                .body("Failed to encode image")
        }
    }
}

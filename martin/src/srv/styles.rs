use actix_middleware_etag::Etag;
use actix_web::http::header::ContentType;
use actix_web::middleware::Compress;
use actix_web::web::{Data, Path};
use actix_web::{HttpResponse, route};
use log::error;
use martin_core::styles::StyleSources;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct StyleRequest {
    style_id: String,
}

#[route(
    "/style/{style_id}",
    method = "GET",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
async fn get_style_json(path: Path<StyleRequest>, styles: Data<StyleSources>) -> HttpResponse {
    let style_id = &path.style_id;
    let Some(path) = styles.style_json_path(style_id) else {
        return HttpResponse::NotFound()
            .content_type(ContentType::plaintext())
            .body("No such style exists");
    };
    let Ok(style_content) = tokio::fs::read_to_string(&path).await else {
        // the file was likely deleted after martin was launched and collected the file list
        // TODO: change this to a server error and log appropriately once the watch mode is here
        return HttpResponse::NotFound()
            .content_type(ContentType::plaintext())
            .body("No such style exists");
    };
    match serde_json::from_str::<serde_json::Value>(&style_content) {
        Ok(value) => HttpResponse::Ok().json(value),
        Err(e) => {
            error!(
                "Failed to parse style JSON {e:?} for style {style_id} at {:?}",
                path.display()
            );

            HttpResponse::BadRequest()
                .content_type(ContentType::plaintext())
                .body(format!(
                    "The requested style {style_id} is malformed: {e:?}"
                ))
        }
    }
}

#[cfg(all(feature = "rendering", target_os = "linux"))]
#[derive(Deserialize, Debug)]
struct StyleRenderRequest {
    style_id: String,
    z: u8,
    x: u32,
    y: u32,
}

#[cfg(all(feature = "rendering", target_os = "linux"))]
#[route("/style/{style_id}/{z}/{x}/{y}.png", method = "GET")]
async fn get_style_rendered(
    path: Path<StyleRenderRequest>,
    styles: Data<StyleSources>,
) -> HttpResponse {
    let style_id = &path.style_id;
    let Some(style_path) = styles.style_json_path(style_id) else {
        return HttpResponse::NotFound()
            .content_type(ContentType::plaintext())
            .body("No such style exists");
    };
    let Some(xyz) = martin_tile_utils::TileCoord::new_checked(path.z, path.x, path.y) else {
        return HttpResponse::BadRequest()
            .content_type(ContentType::plaintext())
            .body("Invalid tile coordinates for zoom level");
    };
    log::trace!(
        "Rendering style {style_id} ({}) at {xyz}",
        style_path.display()
    );

    let image = styles.render(&style_path, xyz).await;
    HttpResponse::Ok()
        .content_type(ContentType::png())
        .body(image.as_slice().to_owned())
}

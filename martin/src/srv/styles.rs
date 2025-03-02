use crate::styles::StyleSources;
use actix_web::http::header::ContentType;
use actix_web::middleware;
use actix_web::web::{Data, Path};
use actix_web::{route, HttpRequest, HttpResponse};
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct StyleRequest {
    style_id: String,
}

#[route(
    "/style/{style_id}",
    method = "GET",
    wrap = "middleware::Compress::default()"
)]
#[allow(clippy::unused_async)]
async fn get_style_json(
    path: Path<StyleRequest>,
    styles: Data<StyleSources>,
    req: HttpRequest,
) -> HttpResponse {
    let Some(path) = styles.style_json_path(&path.style_id) else {
        return HttpResponse::NotFound()
            .content_type(ContentType::plaintext())
            .body("No such style exists");
    };
    let Ok(file) = actix_files::NamedFile::open(path) else {
        // the file was likely deleted after martin was launched and collected the file list
        // TODO: change this to a server error and log appropriately once the watch mode is here
        return HttpResponse::NotFound()
            .content_type(ContentType::plaintext())
            .body("No such style exists");
    };
    file.use_etag(true)
        .use_last_modified(true)
        .into_response(&req)
}

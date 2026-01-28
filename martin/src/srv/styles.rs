use actix_middleware_etag::Etag;
use actix_web::http::header::{ContentType, LOCATION};
use actix_web::middleware::Compress;
use actix_web::web::{Data, Path};
use actix_web::{HttpResponse, route};
use martin_core::styles::StyleSources;
use serde::Deserialize;
use tracing::error;

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

/// Redirect `/styles/{style_id}` to `/style/{style_id}` (HTTP 301)
/// This handles common pluralization mistakes
#[route("/styles/{style_id}", method = "GET", method = "HEAD")]
pub(crate) async fn redirect_styles(path: Path<StyleRequest>) -> HttpResponse {
    static LAST_WARNING: LazyLock<Mutex<Instant>> = LazyLock::new(|| Mutex::new(Instant::now()));

    let mut warning = LAST_WARNING.lock().await;
    if warning.elapsed() >= Duration::from_hours(1) {
        *warning = Instant::now();
        warn!(
            "Using /fonts/{{fontstack}}/{{start}}-{{end}} endpoint which causes an unnecessary redirect. Use /font/{{fontstack}}/{{start}}-{{end}} directly to avoid extra round-trip latency."
        );
    }

    HttpResponse::MovedPermanently()
        .insert_header((LOCATION, format!("/style/{}", path.style_id)))
        .finish()
}

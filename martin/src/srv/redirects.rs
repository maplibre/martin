use actix_web::http::header::LOCATION;
use actix_web::web::Path;
use actix_web::{HttpRequest, HttpResponse, route};
use serde::Deserialize;

// ============================================================================
// Pluralization Redirects
// ============================================================================
// Redirect handlers for common pluralization mistakes and tile format suffixes.
// All redirects use HTTP 301 (Permanent Redirect) and preserve query strings.

/// Redirect `/styles/{style_id}` to `/style/{style_id}`
#[derive(Deserialize)]
struct StyleRedirectRequest {
    style_id: String,
}

#[route("/styles/{style_id}", method = "GET", method = "HEAD")]
async fn redirect_styles(req: HttpRequest, path: Path<StyleRedirectRequest>) -> HttpResponse {
    redirect_with_query(&format!("/style/{}", path.style_id), req.query_string())
}

/// Redirect `/sprites/{source_ids}.json` to `/sprite/{source_ids}.json`
#[derive(Deserialize)]
struct SpriteJsonRedirectRequest {
    source_ids: String,
}

#[route("/sprites/{source_ids}.json", method = "GET", method = "HEAD")]
async fn redirect_sprites_json(
    req: HttpRequest,
    path: Path<SpriteJsonRedirectRequest>,
) -> HttpResponse {
    redirect_with_query(
        &format!("/sprite/{}.json", path.source_ids),
        req.query_string(),
    )
}

/// Redirect `/sprites/{source_ids}.png` to `/sprite/{source_ids}.png`
#[derive(Deserialize)]
struct SpritePngRedirectRequest {
    source_ids: String,
}

#[route("/sprites/{source_ids}.png", method = "GET", method = "HEAD")]
async fn redirect_sprites_png(
    req: HttpRequest,
    path: Path<SpritePngRedirectRequest>,
) -> HttpResponse {
    redirect_with_query(
        &format!("/sprite/{}.png", path.source_ids),
        req.query_string(),
    )
}

/// Redirect `/sdf_sprites/{source_ids}.json` to `/sdf_sprite/{source_ids}.json`
#[derive(Deserialize)]
struct SdfSpriteJsonRedirectRequest {
    source_ids: String,
}

#[route("/sdf_sprites/{source_ids}.json", method = "GET", method = "HEAD")]
async fn redirect_sdf_sprites_json(
    req: HttpRequest,
    path: Path<SdfSpriteJsonRedirectRequest>,
) -> HttpResponse {
    redirect_with_query(
        &format!("/sdf_sprite/{}.json", path.source_ids),
        req.query_string(),
    )
}

/// Redirect `/sdf_sprites/{source_ids}.png` to `/sdf_sprite/{source_ids}.png`
#[derive(Deserialize)]
struct SdfSpritePngRedirectRequest {
    source_ids: String,
}

#[route("/sdf_sprites/{source_ids}.png", method = "GET", method = "HEAD")]
async fn redirect_sdf_sprites_png(
    req: HttpRequest,
    path: Path<SdfSpritePngRedirectRequest>,
) -> HttpResponse {
    redirect_with_query(
        &format!("/sdf_sprite/{}.png", path.source_ids),
        req.query_string(),
    )
}

/// Redirect `/fonts/{fontstack}/{start}-{end}` to `/font/{fontstack}/{start}-{end}`
#[derive(Deserialize)]
struct FontRedirectRequest {
    fontstack: String,
    start: u32,
    end: u32,
}

#[route("/fonts/{fontstack}/{start}-{end}", method = "GET", method = "HEAD")]
async fn redirect_fonts(req: HttpRequest, path: Path<FontRedirectRequest>) -> HttpResponse {
    redirect_with_query(
        &format!("/font/{}/{}-{}", path.fontstack, path.start, path.end),
        req.query_string(),
    )
}

/// Redirect `/tiles/{source_ids}/{z}/{x}/{y}` to `/{source_ids}/{z}/{x}/{y}`
#[derive(Deserialize)]
struct TilesRedirectRequest {
    source_ids: String,
    z: u8,
    x: u32,
    y: u32,
}

#[route("/tiles/{source_ids}/{z}/{x}/{y}", method = "GET", method = "HEAD")]
async fn redirect_tiles(req: HttpRequest, path: Path<TilesRedirectRequest>) -> HttpResponse {
    redirect_with_query(
        &format!("/{}/{}/{}/{}", path.source_ids, path.z, path.x, path.y),
        req.query_string(),
    )
}

// ============================================================================
// Tile Format Suffix Redirects
// ============================================================================

/// Redirect `/{source_ids}/{z}/{x}/{y}.pbf` to `/{source_ids}/{z}/{x}/{y}`
#[derive(Deserialize)]
struct TilePbfRedirectRequest {
    source_ids: String,
    z: u8,
    x: u32,
    y: u32,
}

#[route("/{source_ids}/{z}/{x}/{y}.pbf", method = "GET", method = "HEAD")]
async fn redirect_tile_pbf(req: HttpRequest, path: Path<TilePbfRedirectRequest>) -> HttpResponse {
    redirect_with_query(
        &format!("/{}/{}/{}/{}", path.source_ids, path.z, path.x, path.y),
        req.query_string(),
    )
}

/// Redirect `/{source_ids}/{z}/{x}/{y}.mvt` to `/{source_ids}/{z}/{x}/{y}`
#[derive(Deserialize)]
struct TileMvtRedirectRequest {
    source_ids: String,
    z: u8,
    x: u32,
    y: u32,
}

#[route("/{source_ids}/{z}/{x}/{y}.mvt", method = "GET", method = "HEAD")]
async fn redirect_tile_mvt(req: HttpRequest, path: Path<TileMvtRedirectRequest>) -> HttpResponse {
    redirect_with_query(
        &format!("/{}/{}/{}/{}", path.source_ids, path.z, path.x, path.y),
        req.query_string(),
    )
}

/// Redirect `/{source_ids}/{z}/{x}/{y}.mlt` to `/{source_ids}/{z}/{x}/{y}`
#[derive(Deserialize)]
struct TileMltRedirectRequest {
    source_ids: String,
    z: u8,
    x: u32,
    y: u32,
}

#[route("/{source_ids}/{z}/{x}/{y}.mlt", method = "GET", method = "HEAD")]
async fn redirect_tile_mlt(req: HttpRequest, path: Path<TileMltRedirectRequest>) -> HttpResponse {
    redirect_with_query(
        &format!("/{}/{}/{}/{}", path.source_ids, path.z, path.x, path.y),
        req.query_string(),
    )
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Create a 301 Permanent Redirect response, preserving query strings if present
fn redirect_with_query(target_path: &str, query_string: &str) -> HttpResponse {
    let location = if query_string.is_empty() {
        target_path.to_string()
    } else {
        format!("{target_path}?{query_string}")
    };

    HttpResponse::MovedPermanently()
        .insert_header((LOCATION, location))
        .finish()
}

// ============================================================================
// Public API for registering redirect routes
// ============================================================================

/// Register tile format suffix redirect routes.
/// These MUST be registered BEFORE the main tile route `/{source_ids}/{z}/{x}/{y}`
/// because Actix-Web matches routes in registration order, and more specific
/// patterns need to be registered first.
pub fn register_tile_suffix_redirects(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.service(redirect_tile_pbf)
        .service(redirect_tile_mvt)
        .service(redirect_tile_mlt);
}

/// Register pluralization redirect routes.
/// These should be registered AFTER main routes to act as fallbacks for common mistakes.
pub fn register_pluralization_redirects(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.service(redirect_styles)
        .service(redirect_sprites_json)
        .service(redirect_sprites_png)
        .service(redirect_sdf_sprites_json)
        .service(redirect_sdf_sprites_png)
        .service(redirect_fonts)
        .service(redirect_tiles);
}

/// Register all redirect routes (for backwards compatibility).
/// Prefer using `register_tile_suffix_redirects` and `register_pluralization_redirects` separately.
#[allow(dead_code)]
pub fn register_redirects(cfg: &mut actix_web::web::ServiceConfig) {
    register_tile_suffix_redirects(cfg);
    register_pluralization_redirects(cfg);
}

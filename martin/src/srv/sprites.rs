use std::string::ToString;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use actix_middleware_etag::Etag;
use actix_web::error::ErrorNotFound;
use actix_web::http::header::{ContentType, LOCATION};
use actix_web::middleware::Compress;
use actix_web::web::{Bytes, Data, Path};
use actix_web::{HttpResponse, Result as ActixResult, route};
use martin_core::sprites::{OptSpriteCache, SpriteError, SpriteSources};
use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::warn;

use crate::srv::server::map_internal_error;

#[derive(Deserialize)]
pub struct SourceIDsRequest {
    pub source_ids: String,
}

#[route(
    "/sprite/{source_ids}.png",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()"
)]
async fn get_sprite_png(
    path: Path<SourceIDsRequest>,
    sprites: Data<SpriteSources>,
    cache: Data<OptSpriteCache>,
) -> ActixResult<HttpResponse> {
    let is_sdf = false;
    let png = if let Some(cache) = cache.as_ref() {
        cache
            .get_or_insert(path.source_ids.clone(), is_sdf, false, async || {
                get_sprite(&path.source_ids, &sprites, is_sdf).await
            })
            .await?
    } else {
        get_sprite(&path.source_ids, &sprites, is_sdf).await?
    };
    Ok(HttpResponse::Ok()
        .content_type(ContentType::png())
        .body(png))
}

/// Redirect `/sprites/{source_ids}.png` to `/sprite/{source_ids}.png` (HTTP 301)
#[route("/sprites/{source_ids}.png", method = "GET", method = "HEAD")]
pub async fn redirect_sprites_png(path: Path<SourceIDsRequest>) -> HttpResponse {
    let SourceIDsRequest { source_ids } = path.as_ref();

    static LAST_WARNING: LazyLock<Mutex<Instant>> = LazyLock::new(|| Mutex::new(Instant::now()));

    let mut warning = LAST_WARNING.lock().await;
    if warning.elapsed() >= Duration::from_hours(1) {
        *warning = Instant::now();
        warn!(
            "Using /sprites/{source_ids}.png endpoint which causes an unnecessary redirect. Use /sprite/{source_ids}.png directly to avoid extra round-trip latency."
        );
    }

    HttpResponse::MovedPermanently()
        .insert_header((LOCATION, format!("/sprite/{source_ids}.png")))
        .finish()
}

#[route(
    "/sdf_sprite/{source_ids}.png",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()"
)]
async fn get_sprite_sdf_png(
    path: Path<SourceIDsRequest>,
    sprites: Data<SpriteSources>,
    cache: Data<OptSpriteCache>,
) -> ActixResult<HttpResponse> {
    let is_sdf = true;
    let png = if let Some(cache) = cache.as_ref() {
        cache
            .get_or_insert(path.source_ids.clone(), is_sdf, false, async || {
                get_sprite(&path.source_ids, &sprites, is_sdf).await
            })
            .await?
    } else {
        get_sprite(&path.source_ids, &sprites, is_sdf).await?
    };
    Ok(HttpResponse::Ok()
        .content_type(ContentType::png())
        .body(png))
}

/// Redirect `/sdf_sprites/{source_ids}.png` to `/sdf_sprite/{source_ids}.png` (HTTP 301)
#[route("/sdf_sprites/{source_ids}.png", method = "GET", method = "HEAD")]
pub async fn redirect_sdf_sprites_png(path: Path<SourceIDsRequest>) -> HttpResponse {
    let SourceIDsRequest { source_ids } = path.as_ref();

    static LAST_WARNING: LazyLock<Mutex<Instant>> = LazyLock::new(|| Mutex::new(Instant::now()));

    let mut warning = LAST_WARNING.lock().await;
    if warning.elapsed() >= Duration::from_hours(1) {
        *warning = Instant::now();
        warn!(
            "Using /sdf_sprites/{source_ids}.png endpoint which causes an unnecessary redirect. Use /sdf_sprite/{source_ids}.png directly to avoid extra round-trip latency."
        );
    }

    HttpResponse::MovedPermanently()
        .insert_header((LOCATION, format!("/sdf_sprite/{source_ids}.png")))
        .finish()
}

#[route(
    "/sprite/{source_ids}.json",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
async fn get_sprite_json(
    path: Path<SourceIDsRequest>,
    sprites: Data<SpriteSources>,
    cache: Data<OptSpriteCache>,
) -> ActixResult<HttpResponse> {
    let is_sdf = false;
    let json = if let Some(cache) = cache.as_ref() {
        cache
            .get_or_insert(path.source_ids.clone(), is_sdf, true, async || {
                get_index(&path.source_ids, &sprites, is_sdf).await
            })
            .await?
    } else {
        get_index(&path.source_ids, &sprites, is_sdf).await?
    };
    Ok(HttpResponse::Ok()
        .content_type(ContentType::json())
        .body(json))
}

/// Redirect `/sprites/{source_ids}.json` to `/sprite/{source_ids}.json` (HTTP 301)
#[route("/sprites/{source_ids}.json", method = "GET", method = "HEAD")]
pub async fn redirect_sprites_json(path: Path<SourceIDsRequest>) -> HttpResponse {
    let SourceIDsRequest { source_ids } = path.as_ref();

    static LAST_WARNING: LazyLock<Mutex<Instant>> = LazyLock::new(|| Mutex::new(Instant::now()));

    let mut warning = LAST_WARNING.lock().await;
    if warning.elapsed() >= Duration::from_hours(1) {
        *warning = Instant::now();
        warn!(
            "Using /sprites/{source_ids}.json endpoint which causes an unnecessary redirect. Use /sprite/{source_ids}.json directly to avoid extra round-trip latency."
        );
    }

    HttpResponse::MovedPermanently()
        .insert_header((LOCATION, format!("/sprite/{source_ids}.json")))
        .finish()
}

#[route(
    "/sdf_sprite/{source_ids}.json",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
async fn get_sprite_sdf_json(
    path: Path<SourceIDsRequest>,
    sprites: Data<SpriteSources>,
    cache: Data<OptSpriteCache>,
) -> ActixResult<HttpResponse> {
    let is_sdf = true;
    let json = if let Some(cache) = cache.as_ref() {
        cache
            .get_or_insert(path.source_ids.clone(), is_sdf, true, async || {
                get_index(&path.source_ids, &sprites, is_sdf).await
            })
            .await?
    } else {
        get_index(&path.source_ids, &sprites, is_sdf).await?
    };
    Ok(HttpResponse::Ok()
        .content_type(ContentType::json())
        .body(json))
}

/// Redirect `/sdf_sprites/{source_ids}.json` to `/sdf_sprite/{source_ids}.json` (HTTP 301)
#[route("/sdf_sprites/{source_ids}.json", method = "GET", method = "HEAD")]
pub async fn redirect_sdf_sprites_json(path: Path<SourceIDsRequest>) -> HttpResponse {
    let SourceIDsRequest { source_ids } = path.as_ref();

    static LAST_WARNING: LazyLock<Mutex<Instant>> = LazyLock::new(|| Mutex::new(Instant::now()));

    let mut warning = LAST_WARNING.lock().await;
    if warning.elapsed() >= Duration::from_hours(1) {
        *warning = Instant::now();
        warn!(
            "Using /sdf_sprites/{source_ids}.json endpoint which causes an unnecessary redirect. Use /sdf_sprite/{source_ids}.json directly to avoid extra round-trip latency."
        );
    }

    HttpResponse::MovedPermanently()
        .insert_header((LOCATION, format!("/sdf_sprite/{source_ids}.json")))
        .finish()
}

async fn get_sprite(source_ids: &str, sprites: &SpriteSources, as_sdf: bool) -> ActixResult<Bytes> {
    let sheet = sprites
        .get_sprites(source_ids, as_sdf)
        .await
        .map_err(|e| match e {
            SpriteError::SpriteNotFound(_) => ErrorNotFound(e.to_string()),
            _ => map_internal_error(e),
        })?;
    let json = sheet.encode_png().map_err(map_internal_error)?;
    Ok(Bytes::from(json))
}

async fn get_index(source_ids: &str, sprites: &SpriteSources, as_sdf: bool) -> ActixResult<Bytes> {
    let sheet = sprites
        .get_sprites(source_ids, as_sdf)
        .await
        .map_err(|e| match e {
            SpriteError::SpriteNotFound(_) => ErrorNotFound(e.to_string()),
            _ => map_internal_error(e),
        })?;
    let json = serde_json::to_vec(&sheet.get_index()).map_err(map_internal_error)?;
    Ok(Bytes::from(json))
}

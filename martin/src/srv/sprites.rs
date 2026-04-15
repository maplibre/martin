use std::string::ToString as _;

use actix_middleware_etag::Etag;
use actix_web::error::ErrorNotFound;
use actix_web::http::header::{ContentType, LOCATION};
use actix_web::middleware::Compress;
use actix_web::web::{Bytes, Data, Path};
use actix_web::{HttpResponse, Result as ActixResult, route};
use martin_core::sprites::{OptSpriteCache, SpriteError, SpriteSources};
use serde::Deserialize;
use tracing::warn;

use crate::srv::server::{DebouncedWarning, map_internal_error};

#[derive(thiserror::Error, Debug)]
enum SpriteComputeError {
    #[error(transparent)]
    Sprite(#[from] SpriteError),
    #[error("Failed to encode sprite PNG: {0}")]
    EncodePng(String),
    #[error(transparent)]
    Serialize(#[from] serde_json::Error),
}

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
#[hotpath::measure]
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
            .await
            .map_err(|e| map_sprite_compute_error(e.as_ref()))?
    } else {
        get_sprite(&path.source_ids, &sprites, is_sdf)
            .await
            .map_err(|e| map_sprite_compute_error(&e))?
    };
    Ok(HttpResponse::Ok()
        .content_type(ContentType::png())
        .body(png))
}

/// Redirect `/sprites/{source_ids}.png` to `/sprite/{source_ids}.png` (HTTP 301)
#[route("/sprites/{source_ids}.png", method = "GET", method = "HEAD")]
pub async fn redirect_sprites_png(path: Path<SourceIDsRequest>) -> HttpResponse {
    static WARNING: DebouncedWarning = DebouncedWarning::new();
    let SourceIDsRequest { source_ids } = path.as_ref();

    WARNING
        .once_per_hour(|| {
            warn!(
                "Request to /sprites/{source_ids}.png caused unnecessary redirect. Use /sprite/{source_ids}.png to avoid extra round-trip latency."
            );
        })
        .await;

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
#[hotpath::measure]
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
            .await
            .map_err(|e| map_sprite_compute_error(e.as_ref()))?
    } else {
        get_sprite(&path.source_ids, &sprites, is_sdf)
            .await
            .map_err(|e| map_sprite_compute_error(&e))?
    };
    Ok(HttpResponse::Ok()
        .content_type(ContentType::png())
        .body(png))
}

/// Redirect `/sdf_sprites/{source_ids}.png` to `/sdf_sprite/{source_ids}.png` (HTTP 301)
#[route("/sdf_sprites/{source_ids}.png", method = "GET", method = "HEAD")]
pub async fn redirect_sdf_sprites_png(path: Path<SourceIDsRequest>) -> HttpResponse {
    static WARNING: DebouncedWarning = DebouncedWarning::new();
    let SourceIDsRequest { source_ids } = path.as_ref();

    WARNING
        .once_per_hour(|| {
            warn!(
                "Request to /sdf_sprites/{source_ids}.png caused unnecessary redirect. Use /sdf_sprite/{source_ids}.png to avoid extra round-trip latency."
            );
        })
        .await;

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
#[hotpath::measure]
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
            .await
            .map_err(|e| map_sprite_compute_error(e.as_ref()))?
    } else {
        get_index(&path.source_ids, &sprites, is_sdf)
            .await
            .map_err(|e| map_sprite_compute_error(&e))?
    };
    Ok(HttpResponse::Ok()
        .content_type(ContentType::json())
        .body(json))
}

/// Redirect `/sprites/{source_ids}.json` to `/sprite/{source_ids}.json` (HTTP 301)
#[route("/sprites/{source_ids}.json", method = "GET", method = "HEAD")]
pub async fn redirect_sprites_json(path: Path<SourceIDsRequest>) -> HttpResponse {
    static WARNING: DebouncedWarning = DebouncedWarning::new();
    let SourceIDsRequest { source_ids } = path.as_ref();

    WARNING
        .once_per_hour(|| {
            warn!(
                "Request to /sprites/{source_ids}.json caused unnecessary redirect. Use /sprite/{source_ids}.json to avoid extra round-trip latency."
            );
        })
        .await;

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
#[hotpath::measure]
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
            .await
            .map_err(|e| map_sprite_compute_error(e.as_ref()))?
    } else {
        get_index(&path.source_ids, &sprites, is_sdf)
            .await
            .map_err(|e| map_sprite_compute_error(&e))?
    };
    Ok(HttpResponse::Ok()
        .content_type(ContentType::json())
        .body(json))
}

/// Redirect `/sdf_sprites/{source_ids}.json` to `/sdf_sprite/{source_ids}.json` (HTTP 301)
#[route("/sdf_sprites/{source_ids}.json", method = "GET", method = "HEAD")]
pub async fn redirect_sdf_sprites_json(path: Path<SourceIDsRequest>) -> HttpResponse {
    static WARNING: DebouncedWarning = DebouncedWarning::new();
    let SourceIDsRequest { source_ids } = path.as_ref();

    WARNING
        .once_per_hour(|| {
            warn!(
                "Request to /sdf_sprites/{source_ids}.json caused unnecessary redirect. Use /sdf_sprite/{source_ids}.json to avoid extra round-trip latency."
            );
        })
        .await;

    HttpResponse::MovedPermanently()
        .insert_header((LOCATION, format!("/sdf_sprite/{source_ids}.json")))
        .finish()
}

#[hotpath::measure]
async fn get_sprite(
    source_ids: &str,
    sprites: &SpriteSources,
    as_sdf: bool,
) -> Result<Bytes, SpriteComputeError> {
    let sheet = sprites.get_sprites(source_ids, as_sdf).await?;
    let png = sheet
        .encode_png()
        .map_err(|e| SpriteComputeError::EncodePng(e.to_string()))?;
    Ok(Bytes::from(png))
}

#[hotpath::measure]
async fn get_index(
    source_ids: &str,
    sprites: &SpriteSources,
    as_sdf: bool,
) -> Result<Bytes, SpriteComputeError> {
    let sheet = sprites.get_sprites(source_ids, as_sdf).await?;
    let json = serde_json::to_vec(&sheet.get_index())?;
    Ok(Bytes::from(json))
}

fn map_sprite_compute_error(e: &SpriteComputeError) -> actix_web::Error {
    match e {
        SpriteComputeError::Sprite(err @ SpriteError::SpriteNotFound(_)) => {
            ErrorNotFound(err.to_string())
        }
        other => map_internal_error(other),
    }
}

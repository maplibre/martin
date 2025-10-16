use std::string::ToString;

use actix_middleware_etag::Etag;
use actix_web::error::ErrorNotFound;
use actix_web::http::header::ContentType;
use actix_web::middleware::Compress;
use actix_web::web::{Bytes, Data, Path};
use actix_web::{HttpResponse, Result as ActixResult, route};
use martin_core::sprites::{SpriteError, SpriteSources};
use serde::Deserialize;

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
) -> ActixResult<HttpResponse> {
    let is_sdf = false;
    let png = get_sprite(&path, &sprites, is_sdf).await?;
    Ok(HttpResponse::Ok()
        .content_type(ContentType::png())
        .body(png))
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
) -> ActixResult<HttpResponse> {
    let is_sdf = true;
    let png = get_sprite(&path, &sprites, is_sdf).await?;
    Ok(HttpResponse::Ok()
        .content_type(ContentType::png())
        .body(png))
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
) -> ActixResult<HttpResponse> {
    let is_sdf = false;
    let json = get_index(&path, &sprites, is_sdf).await?;
    Ok(HttpResponse::Ok()
        .content_type(ContentType::json())
        .body(json))
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
) -> ActixResult<HttpResponse> {
    let is_sdf = true;
    let json = get_index(&path, &sprites, is_sdf).await?;
    Ok(HttpResponse::Ok()
        .content_type(ContentType::json())
        .body(json))
}

async fn get_sprite(
    path: &SourceIDsRequest,
    sprites: &SpriteSources,
    as_sdf: bool,
) -> ActixResult<Bytes> {
    let sheet = sprites
        .get_sprites(&path.source_ids, as_sdf)
        .await
        .map_err(|e| match e {
            SpriteError::SpriteNotFound(_) => ErrorNotFound(e.to_string()),
            _ => map_internal_error(e),
        })?;
    let json = sheet.encode_png().map_err(map_internal_error)?;
    Ok(Bytes::from(json))
}

async fn get_index(
    path: &SourceIDsRequest,
    sprites: &SpriteSources,
    as_sdf: bool,
) -> ActixResult<Bytes> {
    let sheet = sprites
        .get_sprites(&path.source_ids, as_sdf)
        .await
        .map_err(|e| match e {
            SpriteError::SpriteNotFound(_) => ErrorNotFound(e.to_string()),
            _ => map_internal_error(e),
        })?;
    let json = serde_json::to_vec(&sheet.get_index()).map_err(map_internal_error)?;
    Ok(Bytes::from(json))
}

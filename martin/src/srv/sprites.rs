use std::string::ToString;

use actix_web::error::ErrorNotFound;
use actix_web::http::header::ContentType;
use actix_web::web::{Data, Path};
use actix_web::{HttpResponse, Result as ActixResult, middleware, route};
use spreet::Spritesheet;

use crate::sprites::{SpriteError, SpriteSources};
use crate::srv::SourceIDsRequest;
use crate::srv::server::map_internal_error;

#[route("/sprite/{source_ids}.png", method = "GET", method = "HEAD")]
async fn get_sprite_png(
    path: Path<SourceIDsRequest>,
    sprites: Data<SpriteSources>,
) -> ActixResult<HttpResponse> {
    let sheet = get_sprite(&path, &sprites, false).await?;
    Ok(HttpResponse::Ok()
        .content_type(ContentType::png())
        .body(sheet.encode_png().map_err(map_internal_error)?))
}

#[route("/sdf_sprite/{source_ids}.png", method = "GET", method = "HEAD")]
async fn get_sprite_sdf_png(
    path: Path<SourceIDsRequest>,
    sprites: Data<SpriteSources>,
) -> ActixResult<HttpResponse> {
    let sheet = get_sprite(&path, &sprites, true).await?;
    Ok(HttpResponse::Ok()
        .content_type(ContentType::png())
        .body(sheet.encode_png().map_err(map_internal_error)?))
}

#[route(
    "/sprite/{source_ids}.json",
    method = "GET",
    method = "HEAD",
    wrap = "middleware::Compress::default()"
)]
async fn get_sprite_json(
    path: Path<SourceIDsRequest>,
    sprites: Data<SpriteSources>,
) -> ActixResult<HttpResponse> {
    let sheet = get_sprite(&path, &sprites, false).await?;
    Ok(HttpResponse::Ok().json(sheet.get_index()))
}

#[route(
    "/sdf_sprite/{source_ids}.json",
    method = "GET",
    method = "HEAD",
    wrap = "middleware::Compress::default()"
)]
async fn get_sprite_sdf_json(
    path: Path<SourceIDsRequest>,
    sprites: Data<SpriteSources>,
) -> ActixResult<HttpResponse> {
    let sheet = get_sprite(&path, &sprites, true).await?;
    Ok(HttpResponse::Ok().json(sheet.get_index()))
}

async fn get_sprite(
    path: &SourceIDsRequest,
    sprites: &SpriteSources,
    as_sdf: bool,
) -> ActixResult<Spritesheet> {
    sprites
        .get_sprites(&path.source_ids, as_sdf)
        .await
        .map_err(|e| match e {
            SpriteError::SpriteNotFound(_) => ErrorNotFound(e.to_string()),
            _ => map_internal_error(e),
        })
}

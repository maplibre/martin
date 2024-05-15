use std::string::ToString;
use std::sync::RwLock;

use actix_web::error::ErrorNotFound;
use actix_web::http::header::ContentType;
use actix_web::web::{Data, Path};
use actix_web::{middleware, route, HttpResponse, Result as ActixResult};
use spreet::Spritesheet;

use crate::sprites::{SpriteError, SpriteSources};
use crate::srv::server::map_internal_error;
use crate::srv::SourceIDsRequest;

#[route("/sprite/{source_ids}.png", method = "GET", method = "HEAD")]
async fn get_sprite_png(
    path: Path<SourceIDsRequest>,
    sprites: Data<RwLock<SpriteSources>>,
) -> ActixResult<HttpResponse> {
    let sprites = sprites.read().map_err(map_internal_error)?; 
    let sheet = get_sprite(&path, &sprites).await?;
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
    sprites: Data<RwLock<SpriteSources>>,
) -> ActixResult<HttpResponse> {
    let sprites = sprites.read().map_err(map_internal_error)?;
    let sheet = get_sprite(&path, &sprites).await?;
    Ok(HttpResponse::Ok().json(sheet.get_index()))
}

async fn get_sprite(path: &SourceIDsRequest, sprites: &SpriteSources) -> ActixResult<Spritesheet> {
    sprites
        .get_sprites(&path.source_ids)
        .await
        .map_err(|e| match e {
            SpriteError::SpriteNotFound(_) => ErrorNotFound(e.to_string()),
            _ => map_internal_error(e),
        })
}

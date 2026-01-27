#![cfg(test)]

use actix_web::http::StatusCode;
use actix_web::test::{TestRequest, call_service};
use indoc::indoc;
use martin::config::file::srv::SrvConfig;

pub mod utils;
pub use utils::*;

macro_rules! create_app {
    ($sources:expr) => {{
        let state = mock_sources(mock_cfg($sources)).await.0;
        ::actix_web::test::init_service(
            ::actix_web::App::new()
                .app_data(actix_web::web::Data::new(
                    ::martin::srv::Catalog::new(&state).unwrap(),
                ))
                .app_data(actix_web::web::Data::new(state.tiles))
                .app_data(actix_web::web::Data::new(
                    ::martin_core::tiles::NO_TILE_CACHE,
                ))
                .app_data(actix_web::web::Data::new(state.sprites))
                .app_data(actix_web::web::Data::new(
                    ::martin_core::sprites::NO_SPRITE_CACHE,
                ))
                .app_data(actix_web::web::Data::new(state.fonts))
                .app_data(actix_web::web::Data::new(
                    ::martin_core::fonts::NO_FONT_CACHE,
                ))
                .app_data(actix_web::web::Data::new(state.styles))
                .app_data(actix_web::web::Data::new(SrvConfig::default()))
                .configure(|c| ::martin::srv::router(c, &SrvConfig::default())),
        )
        .await
    }};
}

fn test_get(path: &str) -> TestRequest {
    TestRequest::get().uri(path)
}

const CONFIG_WITH_STYLES: &str = indoc! {"
    styles:
        sources:
            test_style: ../tests/fixtures/styles/maplibre_demo.json
"};

const CONFIG_WITH_SPRITES: &str = indoc! {"
    sprites:
        sources:
            - ../tests/fixtures/sprites/src1
"};

const CONFIG_WITH_FONTS: &str = indoc! {"
    fonts:
        sources:
            - ../tests/fixtures/fonts
"};

// Helper function to check redirect
async fn assert_redirect(path: &str, expected_location: &str, config: &str) {
    let app = create_app!(config);
    let req = test_get(path).to_request();
    let response = call_service(&app, req).await;

    assert_eq!(
        response.status(),
        StatusCode::MOVED_PERMANENTLY,
        "Expected 301 redirect for path: {}",
        path
    );

    let location = response
        .headers()
        .get("location")
        .expect("Location header should be present")
        .to_str()
        .expect("Location should be valid string");

    assert_eq!(
        location, expected_location,
        "Redirect location mismatch for path: {}",
        path
    );
}

// ============================================================================
// Pluralization Redirect Tests
// ============================================================================

#[actix_rt::test]
#[cfg(feature = "styles")]
async fn test_redirect_styles_to_style() {
    assert_redirect("/styles/test_style", "/style/test_style", CONFIG_WITH_STYLES).await;
}

#[actix_rt::test]
#[cfg(feature = "sprites")]
async fn test_redirect_sprites_json_to_sprite() {
    assert_redirect(
        "/sprites/src1.json",
        "/sprite/src1.json",
        CONFIG_WITH_SPRITES,
    )
    .await;
}

#[actix_rt::test]
#[cfg(feature = "sprites")]
async fn test_redirect_sprites_png_to_sprite() {
    assert_redirect(
        "/sprites/src1.png",
        "/sprite/src1.png",
        CONFIG_WITH_SPRITES,
    )
    .await;
}

#[actix_rt::test]
#[cfg(feature = "sprites")]
async fn test_redirect_sdf_sprites_json_to_sdf_sprite() {
    assert_redirect(
        "/sdf_sprites/src1.json",
        "/sdf_sprite/src1.json",
        CONFIG_WITH_SPRITES,
    )
    .await;
}

#[actix_rt::test]
#[cfg(feature = "sprites")]
async fn test_redirect_sdf_sprites_png_to_sdf_sprite() {
    assert_redirect(
        "/sdf_sprites/src1.png",
        "/sdf_sprite/src1.png",
        CONFIG_WITH_SPRITES,
    )
    .await;
}

#[actix_rt::test]
#[cfg(feature = "fonts")]
async fn test_redirect_fonts_to_font() {
    assert_redirect(
        "/fonts/Noto%20Sans/0-255",
        "/font/Noto%20Sans/0-255",
        CONFIG_WITH_FONTS,
    )
    .await;
}

// ============================================================================
// Tile Format Suffix Redirect Tests
// ============================================================================

#[actix_rt::test]
#[cfg(feature = "mbtiles")]
async fn test_redirect_tile_pbf_suffix() {
    let config = indoc! {"
        mbtiles:
            sources:
                world_cities: ../tests/fixtures/mbtiles/world_cities.mbtiles
    "};
    assert_redirect("/world_cities/0/0/0.pbf", "/world_cities/0/0/0", config).await;
}

#[actix_rt::test]
#[cfg(feature = "mbtiles")]
async fn test_redirect_tile_mvt_suffix() {
    let config = indoc! {"
        mbtiles:
            sources:
                world_cities: ../tests/fixtures/mbtiles/world_cities.mbtiles
    "};
    assert_redirect("/world_cities/0/0/0.mvt", "/world_cities/0/0/0", config).await;
}

#[actix_rt::test]
#[cfg(feature = "mbtiles")]
async fn test_redirect_tile_mlt_suffix() {
    let config = indoc! {"
        mbtiles:
            sources:
                world_cities: ../tests/fixtures/mbtiles/world_cities.mbtiles
    "};
    assert_redirect("/world_cities/0/0/0.mlt", "/world_cities/0/0/0", config).await;
}

#[actix_rt::test]
#[cfg(feature = "mbtiles")]
async fn test_redirect_tiles_prefix() {
    let config = indoc! {"
        mbtiles:
            sources:
                world_cities: ../tests/fixtures/mbtiles/world_cities.mbtiles
    "};
    assert_redirect("/tiles/world_cities/0/0/0", "/world_cities/0/0/0", config).await;
}

// ============================================================================
// Query String Preservation Tests
// ============================================================================

#[actix_rt::test]
#[cfg(feature = "styles")]
async fn test_redirect_preserves_query_string() {
    assert_redirect(
        "/styles/test_style?version=1.0",
        "/style/test_style?version=1.0",
        CONFIG_WITH_STYLES,
    )
    .await;
}

#[actix_rt::test]
#[cfg(feature = "sprites")]
async fn test_redirect_sprites_preserves_query_string() {
    assert_redirect(
        "/sprites/src1.json?scale=2",
        "/sprite/src1.json?scale=2",
        CONFIG_WITH_SPRITES,
    )
    .await;
}

#[actix_rt::test]
#[cfg(feature = "mbtiles")]
async fn test_redirect_tile_preserves_query_string() {
    let config = indoc! {"
        mbtiles:
            sources:
                world_cities: ../tests/fixtures/mbtiles/world_cities.mbtiles
    "};
    assert_redirect(
        "/world_cities/0/0/0.pbf?format=json",
        "/world_cities/0/0/0?format=json",
        config,
    )
    .await;
}

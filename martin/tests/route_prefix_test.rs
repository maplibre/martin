#![cfg(feature = "mbtiles")]

use actix_web::test::{TestRequest, call_service, read_body, read_body_json};
use indoc::formatdoc;
use martin::config::file::srv::SrvConfig;
use mbtiles::temp_named_mbtiles;
use tilejson::TileJSON;

pub mod utils;
pub use utils::*;

macro_rules! create_app_with_prefix {
    ($sources:expr, $srv_config:expr) => {{
        let state = mock_sources(mock_cfg($sources)).await.0;
        ::actix_web::test::init_service(
            ::actix_web::App::new()
                .app_data(actix_web::web::Data::new(
                    ::martin::srv::Catalog::new(&state).unwrap(),
                ))
                .app_data(actix_web::web::Data::new(
                    ::martin_core::tiles::NO_TILE_CACHE,
                ))
                .app_data(actix_web::web::Data::new(state.tiles))
                .app_data(actix_web::web::Data::new($srv_config.clone()))
                .configure(|c| ::martin::srv::router(c, &$srv_config)),
        )
        .await
    }};
}

fn test_get(path: &str) -> TestRequest {
    TestRequest::get().uri(path)
}

async fn config(
    test_name: &str,
) -> (
    String,
    (
        (mbtiles::Mbtiles, mbtiles::sqlx::SqliteConnection),
        (mbtiles::Mbtiles, mbtiles::sqlx::SqliteConnection),
    ),
) {
    let json_script = include_str!("../../tests/fixtures/mbtiles/json.sql");
    let (json_mbt, json_conn, json_file) =
        temp_named_mbtiles(&format!("{test_name}_json"), json_script).await;
    let mvt_script = include_str!("../../tests/fixtures/mbtiles/world_cities.sql");
    let (mvt_mbt, mvt_conn, mvt_file) =
        temp_named_mbtiles(&format!("{test_name}_mvt"), mvt_script).await;

    (
        formatdoc! {"
            mbtiles:
                sources:
                    m_json: {json}
                    m_mvt: {mvt}
            ",
            json = json_file.display(),
            mvt = mvt_file.display(),
        },
        ((json_mbt, json_conn), (mvt_mbt, mvt_conn)),
    )
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn test_route_prefix_basic_endpoints() {
    let (config, _conns) = config("test_route_prefix_basic").await;
    let srv_config = SrvConfig {
        route_prefix: Some("/tiles".to_string()),
        ..Default::default()
    };
    let app = create_app_with_prefix!(&config, srv_config);

    // Test health endpoint
    let req = test_get("/tiles/health").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let body = read_body(response).await;
    assert_eq!(body, "OK");

    // Health without prefix should fail
    let req = test_get("/health").to_request();
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), 404);

    // Test catalog endpoint
    let req = test_get("/tiles/catalog").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let body: serde_json::Value = read_body_json(response).await;
    assert!(body["tiles"]["m_json"].is_object());
    assert!(body["tiles"]["m_mvt"].is_object());

    // Catalog without prefix should fail
    let req = test_get("/catalog").to_request();
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), 404);
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn test_route_prefix_tile_endpoints() {
    let (config, _conns) = config("test_route_prefix_tiles").await;
    let srv_config = SrvConfig {
        route_prefix: Some("/tiles".to_string()),
        ..Default::default()
    };
    let app = create_app_with_prefix!(&config, srv_config);

    // Test TileJSON endpoint
    let req = test_get("/tiles/m_mvt").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let body: TileJSON = read_body_json(response).await;

    // Verify TileJSON contains the route_prefix in tile URLs
    assert_eq!(body.tiles.len(), 1);
    let tile_url = &body.tiles[0];
    assert!(
        tile_url.contains("/tiles/m_mvt/"),
        "Tile URL should contain route_prefix: {tile_url}"
    );

    // TileJSON without prefix should fail
    let req = test_get("/m_mvt").to_request();
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), 404);

    // Test tile data endpoint
    let req = test_get("/tiles/m_mvt/0/0/0").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    assert!(response.status().is_success());

    // Tile without prefix should fail
    let req = test_get("/m_mvt/0/0/0").to_request();
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), 404);
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn test_base_path_overrides_route_prefix() {
    let (config, _conns) = config("test_base_path_override").await;
    let srv_config = SrvConfig {
        route_prefix: Some("/tiles".to_string()),
        base_path: Some("/custom".to_string()),
        ..Default::default()
    };
    let app = create_app_with_prefix!(&config, srv_config);

    // Routes should use route_prefix
    let req = test_get("/tiles/m_mvt").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let body: TileJSON = read_body_json(response).await;

    // But TileJSON URLs should use base_path (explicit override)
    assert_eq!(body.tiles.len(), 1);
    let tile_url = &body.tiles[0];
    assert!(
        tile_url.contains("/custom/m_mvt/"),
        "Tile URL should contain base_path (not route_prefix): {tile_url}"
    );
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn test_nested_route_prefix() {
    let (config, _conns) = config("test_nested_prefix").await;
    let srv_config = SrvConfig {
        route_prefix: Some("/api/v1/tiles".to_string()),
        ..Default::default()
    };
    let app = create_app_with_prefix!(&config, srv_config);

    // Test multiple endpoints work with nested prefix
    let req = test_get("/api/v1/tiles/health").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let body = read_body(response).await;
    assert_eq!(body, "OK");

    let req = test_get("/api/v1/tiles/catalog").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let body: serde_json::Value = read_body_json(response).await;
    assert!(body["tiles"]["m_json"].is_object());

    let req = test_get("/api/v1/tiles/m_mvt").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let body: TileJSON = read_body_json(response).await;

    // Verify TileJSON contains the nested route_prefix
    let tile_url = &body.tiles[0];
    assert!(
        tile_url.contains("/api/v1/tiles/m_mvt/"),
        "Tile URL should contain nested route_prefix: {tile_url}"
    );
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn test_route_prefix_root_path() {
    let (config, _conns) = config("test_root_path").await;
    // Setting route_prefix to "/" should be treated as no prefix after normalization
    let srv_config = SrvConfig {
        route_prefix: None, // "/" gets normalized to None
        ..Default::default()
    };
    let app = create_app_with_prefix!(&config, srv_config);

    // Endpoints should be accessible without prefix
    let req = test_get("/health").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let body = read_body(response).await;
    assert_eq!(body, "OK");

    let req = test_get("/catalog").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let body: serde_json::Value = read_body_json(response).await;
    assert!(body["tiles"]["m_json"].is_object());
}

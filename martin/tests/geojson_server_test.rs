#![cfg(feature = "geojson")]

use actix_web::http::header::{ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_TYPE};
use actix_web::test::{TestRequest, call_service, read_body, read_body_json};
use indoc::indoc;
use insta::assert_yaml_snapshot;
use martin::config::file::srv::SrvConfig;
use martin_tile_utils::decode_gzip;
use tilejson::TileJSON;

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
                .app_data(actix_web::web::Data::new(
                    ::martin_core::tiles::NO_TILE_CACHE,
                ))
                .app_data(actix_web::web::Data::new(state.tiles))
                .app_data(actix_web::web::Data::new(SrvConfig::default()))
                .configure(|c| ::martin::srv::router(c, &SrvConfig::default())),
        )
        .await
    }};
}

fn test_get(path: &str) -> TestRequest {
    TestRequest::get().uri(path)
}

const CONFIG: &str = indoc! {"
        geojson:
            sources:
                geo1: ../tests/fixtures/geojson/feature_collection_1.geojson
                geo2: ../tests/fixtures/geojson/feature_collection_2.geojson
    "};

#[actix_rt::test]
#[tracing_test::traced_test]
async fn geojson_get_catalog() {
    let path = "geojson: ../tests/fixtures/geojson/feature_collection_1.geojson";
    let app = create_app! { path };

    let req = test_get("/catalog").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let body: serde_json::Value = read_body_json(response).await;
    assert_yaml_snapshot!(body, @r"
    fonts: {}
    sprites: {}
    styles: {}
    tiles:
      feature_collection_1:
        content_type: application/x-protobuf
    ");
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn geojson_get_catalog_gzip() {
    let app = create_app! { CONFIG };
    let accept = (ACCEPT_ENCODING, "gzip");
    let req = test_get("/catalog").insert_header(accept).to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let body = decode_gzip(&read_body(response).await).unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_yaml_snapshot!(body, @r"
    fonts: {}
    sprites: {}
    styles: {}
    tiles:
      geo1:
        content_type: application/x-protobuf
      geo2:
        content_type: application/x-protobuf
    ");
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn geojson_get_tilejson() {
    let app = create_app! { CONFIG };
    let req = test_get("/geo1").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let headers = response.headers();
    assert_eq!(headers.get(CONTENT_TYPE).unwrap(), "application/json");
    assert!(headers.get(CONTENT_ENCODING).is_none());
    let body: TileJSON = read_body_json(response).await;
    assert!(body.maxzoom.is_none());
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn geojson_get_tilejson_gzip() {
    let app = create_app! { CONFIG };
    let accept = (ACCEPT_ENCODING, "gzip");
    let req = test_get("/geo1").insert_header(accept).to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let headers = response.headers();
    assert_eq!(headers.get(CONTENT_TYPE).unwrap(), "application/json");
    assert_eq!(headers.get(CONTENT_ENCODING).unwrap(), "gzip");
    let body = decode_gzip(&read_body(response).await).unwrap();
    let body: TileJSON = serde_json::from_slice(body.as_slice()).unwrap();
    assert!(body.maxzoom.is_none());
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn geojson_get_tile() {
    let app = create_app! { CONFIG };
    let req = test_get("/geo1/0/0/0").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/x-protobuf"
    );
    assert!(response.headers().get(CONTENT_ENCODING).is_none());
    let body = read_body(response).await;
    assert!(!body.is_empty());
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn geojson_get_tile_gzip() {
    let app = create_app! { CONFIG };
    let accept = (ACCEPT_ENCODING, "gzip");
    let req = test_get("/geo1/0/0/0").insert_header(accept).to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/x-protobuf"
    );
    // GeoJSON tiles can be gzipped
    let body = read_body(response).await;
    assert!(!body.is_empty());
}

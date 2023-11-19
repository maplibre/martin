use actix_web::http::header::{ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_TYPE};
use actix_web::test::{call_service, read_body, read_body_json, TestRequest};
use ctor::ctor;
use indoc::indoc;
use insta::assert_yaml_snapshot;
use martin::decode_gzip;
use tilejson::TileJSON;

pub mod utils;
pub use utils::*;

#[ctor]
fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

macro_rules! create_app {
    ($sources:expr) => {{
        let state = mock_sources(mock_cfg($sources)).await.0;
        ::actix_web::test::init_service(
            ::actix_web::App::new()
                .app_data(actix_web::web::Data::new(
                    ::martin::srv::Catalog::new(&state).unwrap(),
                ))
                .app_data(actix_web::web::Data::new(state.tiles))
                .configure(::martin::srv::router),
        )
        .await
    }};
}

fn test_get(path: &str) -> TestRequest {
    TestRequest::get().uri(path)
}

const CONFIG: &str = indoc! {"
        pmtiles:
            sources:
                p_png: ../tests/fixtures/pmtiles/stamen_toner__raster_CC-BY+ODbL_z3.pmtiles
    "};

#[actix_rt::test]
async fn pmt_get_catalog() {
    let path = "pmtiles: ../tests/fixtures/pmtiles/stamen_toner__raster_CC-BY+ODbL_z3.pmtiles";
    let app = create_app! { path };

    let req = test_get("/catalog").to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    let body: serde_json::Value = read_body_json(response).await;
    assert_yaml_snapshot!(body, @r###"
    ---
    fonts: {}
    sprites: {}
    tiles:
      stamen_toner__raster_CC-BY-ODbL_z3:
        content_type: image/png
    "###);
}

#[actix_rt::test]
async fn pmt_get_catalog_gzip() {
    let app = create_app! { CONFIG };
    let accept = (ACCEPT_ENCODING, "gzip");
    let req = test_get("/catalog").insert_header(accept).to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    let body = decode_gzip(&read_body(response).await).unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_yaml_snapshot!(body, @r###"
    ---
    fonts: {}
    sprites: {}
    tiles:
      p_png:
        content_type: image/png
    "###);
}

#[actix_rt::test]
async fn pmt_get_tilejson() {
    let app = create_app! { CONFIG };
    let req = test_get("/p_png").to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    let headers = response.headers();
    assert_eq!(headers.get(CONTENT_TYPE).unwrap(), "application/json");
    assert!(headers.get(CONTENT_ENCODING).is_none());
    let body: TileJSON = read_body_json(response).await;
    assert_eq!(body.maxzoom, Some(3));
}

#[actix_rt::test]
async fn pmt_get_tilejson_gzip() {
    let app = create_app! { CONFIG };
    let accept = (ACCEPT_ENCODING, "gzip");
    let req = test_get("/p_png").insert_header(accept).to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    let headers = response.headers();
    assert_eq!(headers.get(CONTENT_TYPE).unwrap(), "application/json");
    assert_eq!(headers.get(CONTENT_ENCODING).unwrap(), "gzip");
    let body = decode_gzip(&read_body(response).await).unwrap();
    let body: TileJSON = serde_json::from_slice(body.as_slice()).unwrap();
    assert_eq!(body.maxzoom, Some(3));
}

#[actix_rt::test]
async fn pmt_get_raster() {
    let app = create_app! { CONFIG };
    let req = test_get("/p_png/0/0/0").to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), "image/png");
    assert!(response.headers().get(CONTENT_ENCODING).is_none());
    let body = read_body(response).await;
    assert_eq!(body.len(), 18404);
}

/// get a raster tile with accepted gzip enc, but should still be non-gzipped
#[actix_rt::test]
async fn pmt_get_raster_gzip() {
    let app = create_app! { CONFIG };
    let accept = (ACCEPT_ENCODING, "gzip");
    let req = test_get("/p_png/0/0/0").insert_header(accept).to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), "image/png");
    assert!(response.headers().get(CONTENT_ENCODING).is_none());
    let body = read_body(response).await;
    assert_eq!(body.len(), 18404);
}

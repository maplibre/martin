use actix_web::http::header::{ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_TYPE};
use actix_web::test::{call_service, read_body, read_body_json, TestRequest};
use ctor::ctor;
use indoc::indoc;
use martin::decode_gzip;
use martin::srv::IndexEntry;
use tilejson::TileJSON;

pub mod utils;
pub use utils::*;

#[ctor]
fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

macro_rules! create_app {
    ($sources:expr) => {{
        let sources = mock_sources(mock_cfg($sources)).await.0;
        let state = crate::utils::mock_app_data(sources).await;
        ::actix_web::test::init_service(
            ::actix_web::App::new()
                .app_data(state)
                .configure(::martin::srv::router),
        )
        .await
    }};
}

fn test_get(path: &str) -> TestRequest {
    TestRequest::get().uri(path)
}

const CONFIG: &str = indoc! {"
        mbtiles:
            sources:
                m_json: tests/fixtures/files/json.mbtiles
                m_mvt: tests/fixtures/files/world_cities.mbtiles
                m_raw_mvt: tests/fixtures/files/uncompressed_mvt.mbtiles
                m_webp: tests/fixtures/files/webp.mbtiles
    "};

#[actix_rt::test]
async fn mbt_get_catalog() {
    let app = create_app! { CONFIG };

    let req = test_get("/catalog").to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    let body = read_body(response).await;
    let sources: Vec<IndexEntry> = serde_json::from_slice(&body).unwrap();
    assert_eq!(sources.iter().filter(|v| v.id == "m_mvt").count(), 1);
    assert_eq!(sources.iter().filter(|v| v.id == "m_webp").count(), 1);
    assert_eq!(sources.iter().filter(|v| v.id == "m_raw_mvt").count(), 1);
}

#[actix_rt::test]
async fn mbt_get_catalog_gzip() {
    let app = create_app! { CONFIG };
    let accept = (ACCEPT_ENCODING, "gzip");
    let req = test_get("/catalog").insert_header(accept).to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    let body = decode_gzip(&read_body(response).await).unwrap();
    let sources: Vec<IndexEntry> = serde_json::from_slice(&body).unwrap();
    assert_eq!(sources.iter().filter(|v| v.id == "m_mvt").count(), 1);
    assert_eq!(sources.iter().filter(|v| v.id == "m_webp").count(), 1);
    assert_eq!(sources.iter().filter(|v| v.id == "m_raw_mvt").count(), 1);
}

#[actix_rt::test]
async fn mbt_get_tilejson() {
    let app = create_app! { CONFIG };
    let req = test_get("/m_mvt").to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    let headers = response.headers();
    assert_eq!(headers.get(CONTENT_TYPE).unwrap(), "application/json");
    assert!(headers.get(CONTENT_ENCODING).is_none());
    let body: TileJSON = read_body_json(response).await;
    assert_eq!(body.maxzoom, Some(6));
}

#[actix_rt::test]
async fn mbt_get_tilejson_gzip() {
    let app = create_app! { CONFIG };
    let accept = (ACCEPT_ENCODING, "gzip");
    let req = test_get("/m_webp").insert_header(accept).to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    let headers = response.headers();
    assert_eq!(headers.get(CONTENT_TYPE).unwrap(), "application/json");
    assert_eq!(headers.get(CONTENT_ENCODING).unwrap(), "gzip");
    let body = decode_gzip(&read_body(response).await).unwrap();
    let body: TileJSON = serde_json::from_slice(body.as_slice()).unwrap();
    assert_eq!(body.maxzoom, Some(0));
}

#[actix_rt::test]
async fn mbt_get_raster() {
    let app = create_app! { CONFIG };
    let req = test_get("/m_webp/0/0/0").to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), "image/webp");
    assert!(response.headers().get(CONTENT_ENCODING).is_none());
    let body = read_body(response).await;
    assert_eq!(body.len(), 11586);
}

/// get a raster tile with accepted gzip enc, but should still be non-gzipped
#[actix_rt::test]
async fn mbt_get_raster_gzip() {
    let app = create_app! { CONFIG };
    let accept = (ACCEPT_ENCODING, "gzip");
    let req = test_get("/m_webp/0/0/0").insert_header(accept).to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), "image/webp");
    assert!(response.headers().get(CONTENT_ENCODING).is_none());
    let body = read_body(response).await;
    assert_eq!(body.len(), 11586);
}

#[actix_rt::test]
async fn mbt_get_mvt() {
    let app = create_app! { CONFIG };
    let req = test_get("/m_mvt/0/0/0").to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/x-protobuf"
    );
    assert!(response.headers().get(CONTENT_ENCODING).is_none());
    let body = read_body(response).await;
    assert_eq!(body.len(), 1828);
}

/// get an MVT tile with accepted gzip enc
#[actix_rt::test]
async fn mbt_get_mvt_gzip() {
    let app = create_app! { CONFIG };
    let accept = (ACCEPT_ENCODING, "gzip");
    let req = test_get("/m_mvt/0/0/0").insert_header(accept).to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/x-protobuf"
    );
    assert_eq!(response.headers().get(CONTENT_ENCODING).unwrap(), "gzip");
    let body = read_body(response).await;
    assert_eq!(body.len(), 1107); // this number could change if compression gets more optimized
    let body = decode_gzip(&body).unwrap();
    assert_eq!(body.len(), 1828);
}

/// get an MVT tile with accepted brotli enc
#[actix_rt::test]
async fn mbt_get_mvt_brotli() {
    let app = create_app! { CONFIG };
    let accept = (ACCEPT_ENCODING, "br");
    let req = test_get("/m_mvt/0/0/0").insert_header(accept).to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/x-protobuf"
    );
    assert_eq!(response.headers().get(CONTENT_ENCODING).unwrap(), "br");
    let body = read_body(response).await;
    assert_eq!(body.len(), 871); // this number could change if compression gets more optimized
    let body = martin::decode_brotli(&body).unwrap();
    assert_eq!(body.len(), 1828);
}

/// get an uncompressed MVT tile
#[actix_rt::test]
async fn mbt_get_raw_mvt() {
    let app = create_app! { CONFIG };
    let req = test_get("/m_raw_mvt/0/0/0").to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/x-protobuf"
    );
    assert!(response.headers().get(CONTENT_ENCODING).is_none());
    let body = read_body(response).await;
    assert_eq!(body.len(), 1828);
}

/// get an uncompressed MVT tile with accepted gzip
#[actix_rt::test]
async fn mbt_get_raw_mvt_gzip() {
    let app = create_app! { CONFIG };
    let accept = (ACCEPT_ENCODING, "gzip");
    let req = test_get("/m_raw_mvt/0/0/0")
        .insert_header(accept)
        .to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/x-protobuf"
    );
    assert_eq!(response.headers().get(CONTENT_ENCODING).unwrap(), "gzip");
    let body = read_body(response).await;
    assert_eq!(body.len(), 1107); // this number could change if compression gets more optimized
    let body = martin::decode_gzip(&body).unwrap();
    assert_eq!(body.len(), 1828);
}

/// get an uncompressed MVT tile with accepted both gzip and brotli enc
#[actix_rt::test]
async fn mbt_get_raw_mvt_gzip_br() {
    let app = create_app! { CONFIG };
    // Sadly, most browsers prefer to ask for gzip - maybe we should force brotli if supported.
    let accept = (ACCEPT_ENCODING, "br, gzip, deflate");
    let req = test_get("/m_raw_mvt/0/0/0")
        .insert_header(accept)
        .to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/x-protobuf"
    );
    assert_eq!(response.headers().get(CONTENT_ENCODING).unwrap(), "br");
    let body = read_body(response).await;
    assert_eq!(body.len(), 871); // this number could change if compression gets more optimized
    let body = martin::decode_brotli(&body).unwrap();
    assert_eq!(body.len(), 1828);
}

/// get a JSON tile
#[actix_rt::test]
async fn mbt_get_json() {
    let app = create_app! { CONFIG };
    let req = test_get("/m_json/0/0/0").to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/json"
    );
    assert!(response.headers().get(CONTENT_ENCODING).is_none());
    let body = read_body(response).await;
    assert_eq!(body.len(), 13);
}

/// get a JSON tile with accepted gzip
#[actix_rt::test]
async fn mbt_get_json_gzip() {
    let app = create_app! { CONFIG };
    let accept = (ACCEPT_ENCODING, "gzip");
    let req = test_get("/m_json/0/0/0").insert_header(accept).to_request();
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/json"
    );
    assert_eq!(response.headers().get(CONTENT_ENCODING).unwrap(), "gzip");
    let body = read_body(response).await;
    assert_eq!(body.len(), 33); // this number could change if compression gets more optimized
    let body = martin::decode_gzip(&body).unwrap();
    assert_eq!(body.len(), 13);
}

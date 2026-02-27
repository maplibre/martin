#![cfg(feature = "mbtiles")]

use actix_web::http::header::{ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_TYPE};
use actix_web::test::{TestRequest, call_service, read_body, read_body_json};
use indoc::formatdoc;
use insta::assert_yaml_snapshot;
use martin::config::file::srv::SrvConfig;
use martin_tile_utils::{decode_brotli, decode_gzip};
use mbtiles::sqlx::SqliteConnection;
use mbtiles::{Mbtiles, temp_named_mbtiles};
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

#[expect(clippy::similar_names)]
async fn config(
    test_name: &str,
) -> (
    String,
    (
        (Mbtiles, SqliteConnection),
        (Mbtiles, SqliteConnection),
        (Mbtiles, SqliteConnection),
        (Mbtiles, SqliteConnection),
        (Mbtiles, SqliteConnection),
    ),
) {
    let json_script = include_str!("../../tests/fixtures/mbtiles/json.sql");
    let (json_mbt, json_conn, json_file) =
        temp_named_mbtiles(&format!("{test_name}_json"), json_script).await;
    let mvt_script = include_str!("../../tests/fixtures/mbtiles/world_cities.sql");
    let (mvt_mbt, mvt_conn, mvt_file) =
        temp_named_mbtiles(&format!("{test_name}_mvt"), mvt_script).await;
    let raw_mvt_script = include_str!("../../tests/fixtures/mbtiles/uncompressed_mvt.sql");
    let (raw_mvt_mbt, raw_mvt_conn, raw_mvt_file) =
        temp_named_mbtiles(&format!("{test_name}_raw_mvt"), raw_mvt_script).await;
    let raw_mlt_script = include_str!("../../tests/fixtures/mbtiles/mlt.sql");
    let (raw_mlt_mbt, raw_mlt_conn, raw_mlt_file) =
        temp_named_mbtiles(&format!("{test_name}_raw_mlt"), raw_mlt_script).await;
    let webp_script = include_str!("../../tests/fixtures/mbtiles/webp-no-primary.sql");
    let (webp_mbt, webp_conn, webp_file) =
        temp_named_mbtiles(&format!("{test_name}_webp"), webp_script).await;

    (
        formatdoc! {"
    mbtiles:
        sources:
            m_json: {json}
            m_mvt: {mvt}
            m_raw_mvt: {raw_mvt}
            m_raw_mlt: {raw_mlt}
            m_webp: {webp}
    ",
        json = json_file.display(),
        mvt = mvt_file.display(),
        raw_mvt = raw_mvt_file.display(),
        raw_mlt = raw_mlt_file.display(),
        webp = webp_file.display()
        },
        (
            (json_mbt, json_conn),
            (mvt_mbt, mvt_conn),
            (raw_mvt_mbt, raw_mvt_conn),
            (raw_mlt_mbt, raw_mlt_conn),
            (webp_mbt, webp_conn),
        ),
    )
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn mbt_get_catalog() {
    let (config, _conns) = config("mbt_get_catalog").await;
    let app = create_app!(&config);
    let req = test_get("/catalog").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let body: serde_json::Value = read_body_json(response).await;
    assert_yaml_snapshot!(body, @r#"
    fonts: {}
    sprites: {}
    styles: {}
    tiles:
      m_json:
        content_type: application/json
        name: Dummy json data
      m_mvt:
        content_encoding: gzip
        content_type: application/x-protobuf
        description: Major cities from Natural Earth data
        name: Major cities from Natural Earth data
      m_raw_mlt:
        attribution: "<a href=\"https://www.openmaptiles.org/\" target=\"_blank\">&copy; OpenMapTiles</a> <a href=\"https://www.openstreetmap.org/copyright\" target=\"_blank\">&copy; OpenStreetMap contributors</a>"
        content_type: application/vnd.maplibre-vector-tile
        description: "A tileset showcasing all layers in OpenMapTiles. https://openmaptiles.org"
        name: OpenMapTiles
      m_raw_mvt:
        content_type: application/x-protobuf
        description: Major cities from Natural Earth data
        name: Major cities from Natural Earth data
      m_webp:
        content_type: image/webp
        name: ne2sr
    "#);
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn mbt_get_catalog_gzip() {
    let (config, _conns) = config("mbt_get_catalog_gzip").await;
    let app = create_app!(&config);
    let accept = (ACCEPT_ENCODING, "gzip");
    let req = test_get("/catalog").insert_header(accept).to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let body = decode_gzip(&read_body(response).await).unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_yaml_snapshot!(body, @r#"
    fonts: {}
    sprites: {}
    styles: {}
    tiles:
      m_json:
        content_type: application/json
        name: Dummy json data
      m_mvt:
        content_encoding: gzip
        content_type: application/x-protobuf
        description: Major cities from Natural Earth data
        name: Major cities from Natural Earth data
      m_raw_mlt:
        attribution: "<a href=\"https://www.openmaptiles.org/\" target=\"_blank\">&copy; OpenMapTiles</a> <a href=\"https://www.openstreetmap.org/copyright\" target=\"_blank\">&copy; OpenStreetMap contributors</a>"
        content_type: application/vnd.maplibre-vector-tile
        description: "A tileset showcasing all layers in OpenMapTiles. https://openmaptiles.org"
        name: OpenMapTiles
      m_raw_mvt:
        content_type: application/x-protobuf
        description: Major cities from Natural Earth data
        name: Major cities from Natural Earth data
      m_webp:
        content_type: image/webp
        name: ne2sr
    "#);
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn mbt_get_tilejson() {
    let (config, _conns) = config("mbt_get_tilejson").await;
    let app = create_app!(&config);
    let req = test_get("/m_mvt").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let headers = response.headers();
    assert_eq!(headers.get(CONTENT_TYPE).unwrap(), "application/json");
    assert!(headers.get(CONTENT_ENCODING).is_none());
    let body: TileJSON = read_body_json(response).await;
    assert_eq!(body.maxzoom, Some(6));
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn mbt_get_tilejson_gzip() {
    let (config, _conns) = config("mbt_get_tilejson_gzip").await;
    let app = create_app!(&config);
    let accept = (ACCEPT_ENCODING, "gzip");
    let req = test_get("/m_webp").insert_header(accept).to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let headers = response.headers();
    assert_eq!(headers.get(CONTENT_TYPE).unwrap(), "application/json");
    assert_eq!(headers.get(CONTENT_ENCODING).unwrap(), "gzip");
    let body = decode_gzip(&read_body(response).await).unwrap();
    let body: TileJSON = serde_json::from_slice(body.as_slice()).unwrap();
    assert_eq!(body.maxzoom, Some(0));
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn mbt_get_raster() {
    let (config, _conns) = config("mbt_get_raster").await;
    let app = create_app!(&config);
    let req = test_get("/m_webp/0/0/0").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), "image/webp");
    assert!(response.headers().get(CONTENT_ENCODING).is_none());
    let body = read_body(response).await;
    assert_eq!(body.len(), 23);
}

/// get a raster tile with accepted gzip enc, but should still be non-gzipped
#[actix_rt::test]
#[tracing_test::traced_test]
async fn mbt_get_raster_gzip() {
    let (config, _conns) = config("mbt_get_raster_gzip").await;
    let app = create_app!(&config);
    let accept = (ACCEPT_ENCODING, "gzip");
    let req = test_get("/m_webp/0/0/0").insert_header(accept).to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), "image/webp");
    assert!(response.headers().get(CONTENT_ENCODING).is_none());
    let body = read_body(response).await;
    assert_eq!(body.len(), 23);
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn mbt_get_mvt() {
    let (config, _conns) = config("mbt_get_mvt").await;
    let app = create_app!(&config);
    let req = test_get("/m_mvt/0/0/0").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    println!("Status = {:?}", response.status());
    println!("Headers = {:?}", response.headers());

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
#[tracing_test::traced_test]
async fn mbt_get_mvt_gzip() {
    let (config, _conns) = config("mbt_get_mvt_gzip").await;
    let app = create_app!(&config);
    let accept = (ACCEPT_ENCODING, "gzip");
    let req = test_get("/m_mvt/0/0/0").insert_header(accept).to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
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
#[tracing_test::traced_test]
async fn mbt_get_mvt_brotli() {
    let (config, _conns) = config("mbt_get_mvt_brotli").await;
    let app = create_app!(&config);
    let accept = (ACCEPT_ENCODING, "br");
    let req = test_get("/m_mvt/0/0/0").insert_header(accept).to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/x-protobuf"
    );
    assert_eq!(response.headers().get(CONTENT_ENCODING).unwrap(), "br");
    let body = read_body(response).await;
    assert_eq!(body.len(), 871); // this number could change if compression gets more optimized
    let body = decode_brotli(&body).unwrap();
    assert_eq!(body.len(), 1828);
}

/// get an uncompressed MVT tile
#[actix_rt::test]
#[tracing_test::traced_test]
async fn mbt_get_raw_mvt() {
    let (config, _conns) = config("mbt_get_raw_mvt").await;
    let app = create_app!(&config);
    let req = test_get("/m_raw_mvt/0/0/0").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/x-protobuf"
    );
    assert!(response.headers().get(CONTENT_ENCODING).is_none());
    let body = read_body(response).await;
    assert_eq!(body.len(), 2);
}

/// get an uncompressed MLT tile
#[actix_rt::test]
#[tracing_test::traced_test]
async fn mbt_get_raw_mlt() {
    let (config, _conns) = config("mbt_get_raw_mlt").await;
    let app = create_app!(&config);
    let req = test_get("/m_raw_mlt/0/0/0").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/vnd.maplibre-vector-tile"
    );
    assert_eq!(response.headers().get(CONTENT_ENCODING), None);
    let body = read_body(response).await;
    assert_eq!(body.iter().as_slice(), &[0x02, 0x01]);
}

/// get an uncompressed MVT tile with accepted gzip
#[actix_rt::test]
#[tracing_test::traced_test]
async fn mbt_get_raw_mvt_gzip() {
    let (config, _conns) = config("mbt_get_raw_mvt_gzip").await;
    let app = create_app!(&config);
    let accept = (ACCEPT_ENCODING, "gzip");
    let req = test_get("/m_raw_mvt/0/0/0")
        .insert_header(accept)
        .to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/x-protobuf"
    );
    assert_eq!(response.headers().get(CONTENT_ENCODING).unwrap(), "gzip");
    let body = read_body(response).await;
    assert_eq!(body.len(), 22); // this number could change if compression gets more optimized
    let body = decode_gzip(&body).unwrap();
    assert_eq!(body.len(), 2);
}

/// get an uncompressed MVT tile with accepted both gzip and brotli enc
#[actix_rt::test]
#[tracing_test::traced_test]
async fn mbt_get_raw_mvt_gzip_br() {
    let (config, _conns) = config("mbt_get_raw_mvt_gzip_br").await;
    let app = create_app!(&config);
    // Sadly, most browsers prefer to ask for gzip - maybe we should force brotli if supported.
    let accept = (ACCEPT_ENCODING, "br, gzip, deflate");
    let req = test_get("/m_raw_mvt/0/0/0")
        .insert_header(accept)
        .to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/x-protobuf"
    );
    assert_eq!(response.headers().get(CONTENT_ENCODING).unwrap(), "gzip");
    let body = read_body(response).await;
    assert_eq!(body.len(), 22); // this number could change if compression gets more optimized
    let body = decode_gzip(&body).unwrap();
    assert_eq!(body.len(), 2);
}

/// get a JSON tile
#[actix_rt::test]
#[tracing_test::traced_test]
async fn mbt_get_json() {
    let (config, _conns) = config("mbt_get_json").await;
    let app = create_app!(&config);
    let req = test_get("/m_json/0/0/0").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
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
#[tracing_test::traced_test]
async fn mbt_get_json_gzip() {
    let (config, _conns) = config("mbt_get_json_gzip").await;
    let app = create_app!(&config);
    let accept = (ACCEPT_ENCODING, "gzip");
    let req = test_get("/m_json/0/0/0").insert_header(accept).to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/json"
    );
    assert_eq!(response.headers().get(CONTENT_ENCODING).unwrap(), "gzip");
    let body = read_body(response).await;
    assert_eq!(body.len(), 33); // this number could change if compression gets more optimized
    let body = decode_gzip(&body).unwrap();
    assert_eq!(body.len(), 13);
}

#![cfg(all(feature = "passthrough", feature = "mlt"))]

//! End-to-end tests for the `passthrough` tile source driven through martin's HTTP API
//! against a mock upstream tile server ([`wiremock`]).

use actix_web::http::header::{
    ACCEPT, ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_TYPE, ETAG, IF_NONE_MATCH,
};
use actix_web::test::{TestRequest, call_service, read_body};
use indoc::formatdoc;
use martin::srv::Catalog;
use martin_tile_utils::encode_gzip;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub mod utils;
pub use utils::*;

/// Builds a martin test app from a config YAML string, resolving sources first.
macro_rules! create_app {
    ($sources:expr) => {{
        let state = mock_sources(mock_cfg($sources)).await.0;
        ::actix_web::test::init_service(
            ::actix_web::App::new()
                .app_data(actix_web::web::Data::new(
                    Catalog::new(
                        #[cfg(any(feature = "sprites", feature = "fonts", feature = "styles"))]
                        &state,
                    )
                    .unwrap(),
                ))
                .app_data(actix_web::web::Data::new(state.tile_manager))
                .app_data(actix_web::web::Data::new(
                    ::martin::config::file::srv::SrvConfig::default(),
                ))
                .configure(|c| {
                    ::martin::srv::router(c, &::martin::config::file::srv::SrvConfig::default())
                }),
        )
        .await
    }};
}

/// A minimal but valid MVT tile: one layer with one point feature.
fn mvt_tile() -> Vec<u8> {
    use mlt_core::geo_types::{Geometry, Point};
    use mlt_core::mvt::tile_layers_to_mvt;
    use mlt_core::{PropKind, PropValue, TileLayer};

    let mut builder = TileLayer::builder("test", 4096).expect("layer builder");
    let name_key = builder
        .add_property("name", PropKind::Str)
        .expect("add property");
    {
        let mut feature = builder.feature(Geometry::Point(Point::new(100, 200)));
        feature.id(Some(1));
        feature
            .property(name_key, PropValue::Str(Some("hello".to_string())))
            .expect("set property");
        feature.finish().expect("finish feature");
    }
    tile_layers_to_mvt(vec![builder.finish()]).expect("encode MVT")
}

/// Config for a single template passthrough source named `proxy` pointing at `server`.
fn template_config(server: &MockServer) -> String {
    let url = format!("{}/{{z}}/{{x}}/{{y}}.pbf", server.uri());
    formatdoc! {"
        passthrough:
          sources:
            proxy: \"{url}\"
    "}
}

/// Mounts a 200-OK MVT response with the given `ETag` at `/0/0/0.pbf`.
async fn mount_mvt(server: &MockServer, etag: &str, body: Vec<u8>) {
    Mock::given(method("GET"))
        .and(path("/0/0/0.pbf"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("ETag", etag)
                .insert_header("Content-Type", "application/x-protobuf")
                .set_body_bytes(body),
        )
        .mount(server)
        .await;
}

#[actix_rt::test]
async fn mvt_passes_through_unchanged() {
    let server = MockServer::start().await;
    let mvt = mvt_tile();
    mount_mvt(&server, "up-etag", mvt.clone()).await;

    let app = create_app!(&template_config(&server));
    let req = TestRequest::get().uri("/proxy/0/0/0").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;

    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/x-protobuf"
    );
    assert_eq!(response.headers().get(ETAG).unwrap(), "\"up-etag\"");
    assert!(response.headers().get(CONTENT_ENCODING).is_none());
    let body = read_body(response).await;
    assert_eq!(body, mvt.as_slice());
}

/// A real upstream (e.g. another Martin) sends the `ETag` quoted on the wire. That quoted value is
/// unusable as a strong tag, so the served tile falls back to a content hash rather than the upstream tag.
#[actix_rt::test]
async fn quoted_upstream_etag_falls_back_to_hash() {
    let server = MockServer::start().await;
    mount_mvt(&server, "\"real-etag\"", mvt_tile()).await;

    let app = create_app!(&template_config(&server));
    let req = TestRequest::get().uri("/proxy/0/0/0").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;

    assert_eq!(
        response.headers().get(ETAG).unwrap(),
        "\"HOYqgtryvtVnHNqNJcg5ug\""
    );
}

#[actix_rt::test]
async fn mlt_conversion_via_accept_header() {
    let server = MockServer::start().await;
    mount_mvt(&server, "up-etag", mvt_tile()).await;

    let app = create_app!(&template_config(&server));
    let req = TestRequest::get()
        .uri("/proxy/0/0/0")
        .insert_header((ACCEPT, "application/vnd.maplibre-tile"))
        .to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;

    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/vnd.maplibre-tile"
    );
    // The converted tile carries a hash-free, format-suffixed etag derived from the upstream one.
    assert_eq!(response.headers().get(ETAG).unwrap(), "\"up-etag+mlt\"");
    let body = read_body(response).await;
    assert!(!body.is_empty(), "converted MLT body should not be empty");
}

#[actix_rt::test]
async fn if_none_match_returns_304() {
    let server = MockServer::start().await;
    mount_mvt(&server, "up-etag", mvt_tile()).await;

    let app = create_app!(&template_config(&server));
    let req = TestRequest::get()
        .uri("/proxy/0/0/0")
        .insert_header((IF_NONE_MATCH, "\"up-etag\""))
        .to_request();
    let response = call_service(&app, req).await;
    assert_eq!(response.status().as_u16(), 304);
}

#[actix_rt::test]
async fn upstream_404_becomes_204() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/0/0/0.pbf"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let app = create_app!(&template_config(&server));
    let req = TestRequest::get().uri("/proxy/0/0/0").to_request();
    let response = call_service(&app, req).await;
    assert_eq!(response.status().as_u16(), 204);
}

#[actix_rt::test]
async fn gzip_upstream_served_verbatim() {
    let server = MockServer::start().await;
    let gzipped = encode_gzip(&mvt_tile()).expect("gzip MVT");
    Mock::given(method("GET"))
        .and(path("/0/0/0.pbf"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("ETag", "gz-etag")
                .insert_header("Content-Type", "application/x-protobuf")
                .insert_header("Content-Encoding", "gzip")
                .set_body_bytes(gzipped.clone()),
        )
        .mount(&server)
        .await;

    let app = create_app!(&template_config(&server));
    // The client must accept gzip, otherwise martin transparently decompresses before serving.
    let req = TestRequest::get()
        .uri("/proxy/0/0/0")
        .insert_header((ACCEPT_ENCODING, "gzip"))
        .to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    assert_eq!(
        response.headers().get(CONTENT_ENCODING).unwrap(),
        "gzip",
        "upstream encoding must be preserved"
    );
    let body = read_body(response).await;
    assert_eq!(
        body,
        gzipped.as_slice(),
        "gzip bytes must be served verbatim"
    );
}

#[actix_rt::test]
async fn cache_hit_avoids_second_upstream_fetch() {
    let server = MockServer::start().await;
    mount_mvt(&server, "up-etag", mvt_tile()).await;

    let app = create_app!(&template_config(&server));
    for _ in 0..2 {
        let req = TestRequest::get().uri("/proxy/0/0/0").to_request();
        let response = call_service(&app, req).await;
        assert_response(response).await;
    }

    let requests = server
        .received_requests()
        .await
        .expect("recording is enabled");
    let tile_requests = requests
        .iter()
        .filter(|r| r.url.path() == "/0/0/0.pbf")
        .count();
    assert_eq!(
        tile_requests, 1,
        "the second request must be served from cache, not refetched"
    );
}

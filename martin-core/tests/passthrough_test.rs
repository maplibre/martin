#![cfg(feature = "passthrough")]
#![allow(clippy::unwrap_used)]

use martin_core::tiles::Source as _;
use martin_core::tiles::passthrough::{PassthroughConfig, PassthroughSource};
use martin_tile_utils::{Encoding, Format, TileCoord};
use rstest::rstest;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn coord(z: u8, x: u32, y: u32) -> TileCoord {
    TileCoord::new_unchecked(z, x, y)
}

fn template_config(server: &MockServer, format: Option<Format>) -> PassthroughConfig {
    PassthroughConfig {
        urls: vec![format!("{}/{{z}}/{{x}}/{{y}}.pbf", server.uri())],
        format,
        ..Default::default()
    }
}

#[tokio::test]
async fn serves_tile_bytes_with_detected_format() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/0/0/0.pbf"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/x-protobuf")
                .set_body_bytes(b"tile-bytes".as_ref()),
        )
        .mount(&server)
        .await;

    let src = PassthroughSource::new("t".into(), template_config(&server, None))
        .await
        .unwrap();
    let tile = src.get_tile_with_etag(coord(0, 0, 0), None).await.unwrap();

    assert_eq!(tile.data, b"tile-bytes");
    assert_eq!(tile.info.format, Format::Mvt);
    assert_eq!(tile.info.encoding, Encoding::Uncompressed);
}

#[rstest]
#[case::not_found(404)]
#[case::no_content(204)]
#[tokio::test]
async fn empty_status_yields_empty_tile(#[case] status: u16) {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(status))
        .mount(&server)
        .await;
    let src = PassthroughSource::new("t".into(), template_config(&server, Some(Format::Mvt)))
        .await
        .unwrap();
    let tile = src.get_tile_with_etag(coord(0, 0, 0), None).await.unwrap();
    assert!(tile.is_empty());
}

#[tokio::test]
async fn server_error_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let src = PassthroughSource::new("t".into(), template_config(&server, Some(Format::Mvt)))
        .await
        .unwrap();
    src.get_tile(coord(0, 0, 0), None).await.unwrap_err();
}

#[tokio::test]
async fn preserves_upstream_content_encoding_verbatim() {
    let server = MockServer::start().await;
    let body = b"\x1f\x8b\x08\x00compressed".to_vec();
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/x-protobuf")
                .insert_header("content-encoding", "gzip")
                .set_body_bytes(body.clone()),
        )
        .mount(&server)
        .await;
    let src = PassthroughSource::new("t".into(), template_config(&server, Some(Format::Mvt)))
        .await
        .unwrap();
    let tile = src.get_tile_with_etag(coord(0, 0, 0), None).await.unwrap();

    assert_eq!(tile.info.encoding, Encoding::Gzip);
    assert_eq!(tile.data, body, "bytes must not be decompressed");
}

#[tokio::test]
async fn uses_upstream_etag_verbatim_else_hashes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/1/0/0.pbf"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"upstream-tag\"")
                .set_body_bytes(b"a".as_ref()),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/2/0/0.pbf"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"b".as_ref()))
        .mount(&server)
        .await;
    let src = PassthroughSource::new("t".into(), template_config(&server, Some(Format::Mvt)))
        .await
        .unwrap();

    let with_etag = src.get_tile_with_etag(coord(1, 0, 0), None).await.unwrap();
    assert_eq!(with_etag.etag, "\"upstream-tag\"");

    let hashed = src.get_tile_with_etag(coord(2, 0, 0), None).await.unwrap();
    assert!(!hashed.etag.is_empty());
    assert_ne!(hashed.etag, "\"upstream-tag\"");
}

#[tokio::test]
async fn discovers_templates_from_tilejson() {
    let server = MockServer::start().await;
    let tiles_url = format!("{}/{{z}}/{{x}}/{{y}}.pbf", server.uri());
    let doc = serde_json::json!({
        "tilejson": "3.0.0",
        "tiles": [tiles_url],
        "minzoom": 2,
        "maxzoom": 7,
    });
    Mock::given(method("GET"))
        .and(path("/tiles.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(doc))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/3/1/2.pbf"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"vt".as_ref()))
        .mount(&server)
        .await;

    let config = PassthroughConfig {
        urls: vec![format!("{}/tiles.json", server.uri())],
        ..Default::default()
    };
    let src = PassthroughSource::new("t".into(), config).await.unwrap();
    assert_eq!(src.get_tilejson().minzoom, Some(2));
    assert_eq!(src.get_tilejson().maxzoom, Some(7));

    let tile = src.get_tile(coord(3, 1, 2), None).await.unwrap();
    assert_eq!(tile, b"vt");
}

#[tokio::test]
async fn issues_exactly_one_request_per_tile() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/0/0/0.pbf"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"x".as_ref()))
        .expect(1)
        .mount(&server)
        .await;
    let src = PassthroughSource::new("t".into(), template_config(&server, Some(Format::Mvt)))
        .await
        .unwrap();
    src.get_tile(coord(0, 0, 0), None).await.unwrap();
}

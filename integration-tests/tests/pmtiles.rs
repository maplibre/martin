//! `PMTiles` sources, both configured up front and served from a watched directory.

use std::fs;

use martin_integration_tests::{Martin, WatchedDir, fixture};

#[tokio::test]
async fn auto_configured_minimal() {
    let tmp = tempfile::tempdir().expect("failed to create a temp dir");
    let save_config = tmp.path().join("save_config.yaml");
    let mut martin = Martin::builder()
        .arg("--save-config")
        .arg(&save_config)
        .arg("tests/fixtures/pmtiles2")
        .start()
        .await
        .expect("failed to start martin");

    let catalog = martin.get("/catalog").await;
    assert_eq!(catalog.status(), 200);
    insta::assert_snapshot!(catalog.headers_snapshot(), @r"
    content-encoding: br
    content-type: application/json
    transfer-encoding: chunked
    vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    ");

    let mut catalog_json = catalog.json();
    let settings = catalog_json
        .as_object_mut()
        .expect("catalog is a json object")
        .remove("settings")
        .expect("catalog has a settings key");
    #[cfg(target_os = "linux")]
    assert_eq!(settings, serde_json::json!({ "rendering": false }));
    #[cfg(not(target_os = "linux"))]
    assert_eq!(settings, serde_json::json!({}));

    let mut insta_settings = insta::Settings::clone_current();
    insta_settings.set_sort_maps(true);
    insta_settings.bind(|| {
        insta::assert_json_snapshot!(catalog_json, @r#"
        {
          "fonts": {},
          "sprites": {},
          "styles": {},
          "tiles": {
            "webp2": {
              "content_type": "image/webp",
              "name": "ne2sr"
            }
          }
        }
        "#);
    });

    let tile = martin.get("/webp2/0/0/0").await;
    assert_eq!(tile.status(), 200);
    insta::assert_snapshot!(tile.headers_snapshot(), @r#"
    content-length: 11586
    content-type: image/webp
    etag: "wutUPc_mx5TO8aNmMnsK8A"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    assert!(
        tile.body().starts_with(b"RIFF"),
        "expected a webp (RIFF) tile body"
    );

    let saved = fs::read_to_string(&save_config).expect("martin did not write --save-config");
    insta::assert_snapshot!(saved, @r"
    listen_addresses: 127.0.0.1:0
    pmtiles:
      paths: tests/fixtures/pmtiles2
      sources:
        webp2: tests/fixtures/pmtiles2/webp2.pmtiles
    mbtiles: tests/fixtures/pmtiles2
    geojson: tests/fixtures/pmtiles2
    ");

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn route_prefix_keeps_root_health() {
    let mut martin = Martin::builder()
        .arg("--route-prefix")
        .arg("/foo")
        .arg("tests/fixtures/pmtiles2")
        .start()
        .await
        .expect("failed to start martin");

    let root = martin.get("/health").await;
    assert_eq!(root.status(), 200);
    assert_eq!(root.text(), "OK");

    let prefixed = martin.get("/foo/health").await;
    assert_eq!(prefixed.status(), 200);
    assert_eq!(prefixed.text(), "OK");

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn reload_adds_updates_and_removes_a_source() {
    let watched = WatchedDir::new();
    let mut martin = Martin::builder()
        .arg(watched.dir())
        .start()
        .await
        .expect("failed to start martin");

    let catalog = martin.get("/catalog").await;
    assert_eq!(catalog.json()["tiles"], serde_json::json!({}));
    insta::assert_snapshot!(catalog.headers_snapshot(), @r"
    content-encoding: br
    content-type: application/json
    transfer-encoding: chunked
    vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    ");
    assert_eq!(martin.get("/png/0/0/0").await.status(), 404);

    watched.install(fixture("pmtiles/png.pmtiles"), "png.pmtiles");
    martin.wait_for_source("png").await;
    insta::assert_json_snapshot!(martin.get("/catalog").await.json()["tiles"], @r#"
    {
      "png": {
        "content_type": "image/png",
        "name": "ne2sr"
      }
    }
    "#);

    let tile = martin.get("/png/0/0/0").await;
    assert_eq!(tile.status(), 200);
    assert!(
        tile.body().starts_with(b"\x89PNG"),
        "expected a png tile body"
    );

    watched.touch("png.pmtiles");
    martin.wait_for_log("Updated source source.id=png").await;

    watched.remove("png.pmtiles");
    martin.wait_for_source_removed("png").await;
    assert_eq!(martin.get("/png/0/0/0").await.status(), 404);

    martin.stop().await;
    martin.assert_log_contains("Added source source.id=png");
    martin.assert_log_contains("Updated source source.id=png");
    martin.assert_log_contains("Removed source source.id=png");
    martin.assert_log_contains(r#"ERROR error="Source png does not exist""#);
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn reload_removes_a_source_present_at_startup() {
    let watched = WatchedDir::new();
    watched.seed(fixture("pmtiles/png.pmtiles"), "png.pmtiles");

    let mut martin = Martin::builder()
        .arg(watched.dir())
        .start()
        .await
        .expect("failed to start martin");

    martin.wait_for_source("png").await;
    assert_eq!(martin.get("/png/0/0/0").await.status(), 200);

    watched.remove("png.pmtiles");
    martin.wait_for_source_removed("png").await;

    martin.stop().await;
    martin.assert_log_contains("Removed source source.id=png");
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

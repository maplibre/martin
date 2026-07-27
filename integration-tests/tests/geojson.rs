//! `GeoJSON` sources served from a watched directory.

use martin_integration_tests::{Martin, WatchedDir, fixture};

#[tokio::test]
async fn reload_adds_updates_and_removes_a_source() {
    let watched = WatchedDir::new();
    let mut martin = Martin::builder()
        .arg(watched.dir())
        .start()
        .await
        .expect("failed to start martin");

    assert_eq!(
        martin.get("/catalog").await.json()["tiles"],
        serde_json::json!({})
    );

    watched.install(
        fixture("geojson/feature_collection_1.geojson"),
        "feature_collection_1.geojson",
    );
    martin.wait_for_source("feature_collection_1").await;
    insta::assert_json_snapshot!(martin.get("/catalog").await.json()["tiles"], @r#"
    {
      "feature_collection_1": {
        "content_type": "application/x-protobuf"
      }
    }
    "#);

    let tile = martin.get("/feature_collection_1/0/0/0").await;
    assert_eq!(tile.status(), 200);
    insta::assert_snapshot!(tile.headers_snapshot(), @r#"
    content-encoding: gzip
    content-length: 143
    content-type: application/x-protobuf
    etag: "Wtlvu7ZHlUF7ibfKmKKoag"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    let layers = tile.mvt().layers;
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].name, "feature_collection_1");
    assert_eq!(layers[0].features.len(), 3);

    watched.touch("feature_collection_1.geojson");
    martin
        .wait_for_log("Updated source source.id=feature_collection_1")
        .await;

    watched.remove("feature_collection_1.geojson");
    martin.wait_for_source_removed("feature_collection_1").await;
    assert_eq!(
        martin.get("/feature_collection_1/0/0/0").await.status(),
        404
    );

    martin.stop().await;
    martin.assert_log_contains("Added source source.id=feature_collection_1");
    martin.assert_log_contains("Updated source source.id=feature_collection_1");
    martin.assert_log_contains("Removed source source.id=feature_collection_1");
    martin.assert_log_contains(r#"ERROR error="Source feature_collection_1 does not exist""#);
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

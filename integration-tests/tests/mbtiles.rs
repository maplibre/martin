//! `MBTiles` sources served from a watched directory.

use martin_integration_tests::{Martin, WatchedDir, fixture, mbtiles_from_sql};

#[tokio::test]
async fn reload_adds_and_updates_a_source() {
    let watched = WatchedDir::new();
    let original = watched.outside("original.mbtiles");
    let modified = watched.outside("modified.mbtiles");
    mbtiles_from_sql(fixture("mbtiles/world_cities.sql"), &original).await;
    mbtiles_from_sql(fixture("mbtiles/world_cities_modified.sql"), &modified).await;

    let mut martin = Martin::builder()
        .arg(watched.dir())
        .start()
        .await
        .expect("failed to start martin");

    assert_eq!(
        martin.get("/catalog").await.json()["tiles"],
        serde_json::json!({})
    );

    watched.install(&original, "world_cities.mbtiles");
    martin.wait_for_source("world_cities").await;
    insta::assert_json_snapshot!(martin.get("/catalog").await.json()["tiles"], @r#"
    {
      "world_cities": {
        "content_encoding": "gzip",
        "content_type": "application/x-protobuf",
        "description": "Major cities from Natural Earth data",
        "name": "Major cities from Natural Earth data"
      }
    }
    "#);

    let tile = martin.get("/world_cities/0/0/0").await;
    assert_eq!(tile.status(), 200);
    insta::assert_snapshot!(tile.headers_snapshot(), @r#"
    content-encoding: gzip
    content-length: 1107
    content-type: application/x-protobuf
    etag: "fZ_WrS_v5P9bJuL6UuRBQQ"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    let layers = tile.mvt().layers;
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].name, "cities");
    assert_eq!(layers[0].features.len(), 68);

    watched.install(&modified, "world_cities.mbtiles");
    martin
        .wait_for_log("Updated source source.id=world_cities")
        .await;
    insta::assert_json_snapshot!(martin.get("/catalog").await.json()["tiles"], @r#"
    {
      "world_cities": {
        "content_encoding": "gzip",
        "content_type": "application/x-protobuf",
        "description": "A modified version of major cities from Natural Earth data",
        "name": "Major cities from Natural Earth data"
      }
    }
    "#);

    martin.stop().await;
    martin.assert_log_contains("Added source source.id=world_cities");
    martin.assert_log_contains("Updated source source.id=world_cities");
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[cfg(not(windows))]
#[tokio::test]
async fn reload_removes_a_source_when_its_file_is_deleted() {
    let watched = WatchedDir::new();
    let original = watched.outside("original.mbtiles");
    mbtiles_from_sql(fixture("mbtiles/world_cities.sql"), &original).await;

    let mut martin = Martin::builder()
        .arg(watched.dir())
        .start()
        .await
        .expect("failed to start martin");

    watched.install(&original, "world_cities.mbtiles");
    martin.wait_for_source("world_cities").await;

    watched.remove("world_cities.mbtiles");
    martin.wait_for_source_removed("world_cities").await;
    assert_eq!(martin.get("/world_cities/0/0/0").await.status(), 404);
    assert_eq!(
        martin.get("/catalog").await.json()["tiles"],
        serde_json::json!({})
    );

    martin.stop().await;
    martin.assert_log_contains("Added source source.id=world_cities");
    martin.assert_log_contains("Removed source source.id=world_cities");
    martin.assert_log_contains(r#"ERROR error="Source world_cities does not exist""#);
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

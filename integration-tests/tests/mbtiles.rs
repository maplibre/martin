//! `MBTiles` sources, both configured up front and served from a watched directory.

use martin_integration_tests::{Martin, WatchedDir, fixture, mbtiles_fixture, mbtiles_from_sql};
use rstest::rstest;
use tempfile::TempDir;

async fn martin_serving(args: &[&str], names: &[&str]) -> (TempDir, Martin) {
    let dir = tempfile::tempdir().expect("failed to create a temp dir");
    let mut builder = Martin::builder();
    for arg in args {
        builder = builder.arg(*arg);
    }
    for name in names {
        builder = builder.arg(mbtiles_fixture(dir.path(), name).await);
    }
    let martin = builder.start().await.expect("failed to start martin");
    (dir, martin)
}

async fn tilejson(martin: &Martin, id: &str) -> serde_json::Value {
    let response = martin.get(&format!("/{id}")).await;
    assert_eq!(response.status(), 200);
    serde_json::from_str(&martin.redact(&response.text())).expect("the tilejson is not valid json")
}

#[tokio::test]
async fn a_jpeg_source_serves_its_tilejson_and_tiles() {
    let (_dir, mut martin) = martin_serving(&[], &["geography-class-jpg"]).await;

    let response = martin.get("/geography-class-jpg").await;
    assert_eq!(response.status(), 200);
    insta::with_settings!({filters => vec![(r"(?m)^etag: .*$", "etag: [ETAG]")]}, {
        insta::assert_snapshot!(response.headers_snapshot(), @"
        content-encoding: br
        content-type: application/json
        etag: [ETAG]
        transfer-encoding: chunked
        vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
        ");
    });
    insta::assert_json_snapshot!(tilejson(&martin, "geography-class-jpg").await, @r#"
    {
      "bounds": [
        -180.0,
        -85.0511,
        180.0,
        85.0511
      ],
      "description": "One of the example maps that comes with TileMill - a bright & colorful world map that blends retro and high-tech with its folded paper texture and interactive flag tooltips. ",
      "legend": "<div style=\"text-align:center;\">\n\n<div style=\"font:12pt/16pt Georgia,serif;\">Geography Class</div>\n<div style=\"font:italic 10pt/16pt Georgia,serif;\">by MapBox</div>\n\n<img src=\"data:image/png;base64,iVBORw0KGgo\">\n</div>",
      "maxzoom": 1,
      "minzoom": 0,
      "name": "Geography Class",
      "template": "foobar",
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/geography-class-jpg/{z}/{x}/{y}"
      ],
      "version": "1.0.0"
    }
    "#);

    let tile = martin.get("/geography-class-jpg/0/0/0").await;
    assert_eq!(tile.status(), 200);
    insta::assert_snapshot!(tile.headers_snapshot(), @r#"
    content-length: 6
    content-type: image/jpeg
    etag: "ykC6pGx_UZ3UnUwYbyl2yw"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    assert_eq!(tile.body(), b"\xff\xd8\xff\xff\xff\xd9");

    martin.stop().await;
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_png_source_serves_its_tilejson_and_tiles() {
    let (_dir, mut martin) = martin_serving(&[], &["geography-class-png"]).await;

    insta::assert_json_snapshot!(tilejson(&martin, "geography-class-png").await, @r#"
    {
      "bounds": [
        -180.0,
        -85.0511,
        180.0,
        85.0511
      ],
      "center": [
        0.0,
        20.0,
        0
      ],
      "description": "One of the example maps that comes with TileMill - a bright & colorful world map that blends retro and high-tech with its folded paper texture and interactive flag tooltips. ",
      "legend": "<div style=\"text-align:center;\">\n\n<div style=\"font:12pt/16pt Georgia,serif;\">Geography Class</div>\n<div style=\"font:italic 10pt/16pt Georgia,serif;\">by MapBox</div>\n\n<img src=\"data:image/png;base64,iVBORw0KGgo\">\n</div>",
      "maxzoom": 2,
      "minzoom": 0,
      "name": "Geography Class",
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/geography-class-png/{z}/{x}/{y}"
      ],
      "version": "1.0.0"
    }
    "#);

    let tile = martin.get("/geography-class-png/0/0/0").await;
    assert_eq!(tile.status(), 200);
    insta::assert_snapshot!(tile.headers_snapshot(), @r#"
    content-length: 9
    content-type: image/png
    etag: "AsEwc3Kp5v5Qd7xgb4RDuA"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    assert_eq!(tile.body(), b"\x89PNG\r\n\x1a\n\x01");

    martin.stop().await;
    martin.assert_log_clean();
}

#[tokio::test]
async fn an_mvt_source_serves_its_tilejson() {
    let (_dir, mut martin) = martin_serving(&[], &["world_cities"]).await;

    insta::assert_json_snapshot!(tilejson(&martin, "world_cities").await, @r#"
    {
      "bounds": [
        -123.12359,
        -37.818085,
        174.763027,
        59.352706
      ],
      "center": [
        -75.9375,
        38.788894,
        6
      ],
      "description": "Major cities from Natural Earth data",
      "format": "pbf",
      "maxzoom": 6,
      "minzoom": 0,
      "name": "Major cities from Natural Earth data",
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/world_cities/{z}/{x}/{y}"
      ],
      "vector_layers": [
        {
          "description": "",
          "fields": {
            "name": "String"
          },
          "id": "cities",
          "maxzoom": 6,
          "minzoom": 0
        }
      ],
      "version": "2"
    }
    "#);

    martin.stop().await;
    martin.assert_log_clean();
}

#[tokio::test]
async fn an_mvt_source_serves_a_decodable_tile() {
    let (_dir, mut martin) = martin_serving(&[], &["world_cities"]).await;

    let tile = martin.get("/world_cities/2/3/1").await;
    assert_eq!(tile.status(), 200);
    insta::assert_snapshot!(tile.headers_snapshot(), @r#"
    content-encoding: gzip
    content-length: 263
    content-type: application/x-protobuf
    etag: "nQg6luGC0-kziK3j3sM-zA"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    insta::assert_snapshot!(tile.mvt_dump(), @r#"
    layer: 0
      name: cities
      version: 2
      extent: 4096
      feature: 0
        id: (none)
        geometry: POINT(-77,3044)
        properties:
          name = "Kolkata"
      feature: 1
        id: (none)
        geometry: POINT(640,2628)
        properties:
          name = "Chengdu"
      feature: 2
        id: (none)
        geometry: POINT(478,3464)
        properties:
          name = "Bangkok"
      feature: 3
        id: (none)
        geometry: POINT(630,4037)
        properties:
          name = "Singapore"
      feature: 4
        id: (none)
        geometry: POINT(1200,2110)
        properties:
          name = "Beijing"
      feature: 5
        id: (none)
        geometry: POINT(1100,3054)
        properties:
          name = "Hong Kong"
      feature: 6
        id: (none)
        geometry: POINT(1430,2599)
        properties:
          name = "Shanghai"
      feature: 7
        id: (none)
        geometry: POINT(1436,2918)
        properties:
          name = "Taipei"
      feature: 8
        id: (none)
        geometry: POINT(1683,2248)
        properties:
          name = "Seoul"
      feature: 9
        id: (none)
        geometry: POINT(1409,3423)
        properties:
          name = "Manila"
      feature: 10
        id: (none)
        geometry: POINT(2068,2407)
        properties:
          name = "Ōsaka"
      feature: 11
        id: (none)
        geometry: POINT(2264,2355)
        properties:
          name = "Tokyo"
    "#);

    martin.stop().await;
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_normalized_source_serves_its_tilejson() {
    let (_dir, mut martin) = martin_serving(&[], &["normalized-dedup-id"]).await;

    insta::assert_json_snapshot!(tilejson(&martin, "normalized-dedup-id").await, @r#"
    {
      "bounds": [
        -180.0,
        -85.0511,
        180.0,
        85.0511
      ],
      "description": "Test fixture for normalized schema with integer tile_data_id",
      "format": "jpeg",
      "maxzoom": 1,
      "minzoom": 0,
      "name": "Normalized DedupId Test",
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/normalized-dedup-id/{z}/{x}/{y}"
      ]
    }
    "#);

    let tile = martin.get("/normalized-dedup-id/0/0/0").await;
    assert_eq!(tile.status(), 200);
    insta::assert_snapshot!(tile.headers_snapshot(), @r#"
    content-length: 6
    content-type: image/jpeg
    etag: "ykC6pGx_UZ3UnUwYbyl2yw"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);

    martin.stop().await;
    martin.assert_log_clean();
}

#[rstest]
#[case::z0(0, 0, 0, b"\xff\xd8\xff\xff\xff\xd9".as_slice())]
#[case::z1_bottom_left(1, 0, 0, b"\xff\xd8\xff\xd9".as_slice())]
#[case::z1_top_left(1, 0, 1, b"\xff\xd8\xff\x00\xd9".as_slice())]
#[case::z1_bottom_right(1, 1, 0, b"\xff\xd8\xff\x11\xd9".as_slice())]
#[case::z1_top_right(1, 1, 1, b"\xff\xd8\xff\x00\xff\xd9".as_slice())]
#[tokio::test]
async fn a_normalized_source_resolves_every_deduplicated_tile(
    #[case] z: u8,
    #[case] x: u32,
    #[case] y: u32,
    #[case] expected: &[u8],
) {
    let (_dir, mut martin) =
        martin_serving(&[], &["normalized-dedup-id", "geography-class-jpg"]).await;

    let deduplicated = martin
        .get(&format!("/normalized-dedup-id/{z}/{x}/{y}"))
        .await;
    assert_eq!(deduplicated.status(), 200);
    assert_eq!(deduplicated.body(), expected);

    let flat = martin
        .get(&format!("/geography-class-jpg/{z}/{x}/{y}"))
        .await;
    assert_eq!(
        flat.body(),
        deduplicated.body(),
        "the normalized schema must serve the same tile as the flat one"
    );

    martin.stop().await;
    martin.assert_log_clean();
}

#[rstest]
#[case::a_version_in_the_metadata("geography-class-jpg", "?version=1.0.0")]
#[case::another_version_in_the_metadata("world_cities", "?version=2")]
#[case::no_version_in_the_metadata("normalized-dedup-id", "")]
#[tokio::test]
async fn the_tilejson_url_carries_the_source_version(#[case] id: &str, #[case] query: &str) {
    let (_dir, mut martin) =
        martin_serving(&["--tilejson-url-version-param", "version"], &[id]).await;

    assert_eq!(
        tilejson(&martin, id).await["tiles"][0],
        serde_json::json!(format!("http://[ADDR]/{id}/{{z}}/{{x}}/{{y}}{query}"))
    );

    martin.stop().await;
    martin.assert_log_clean();
}

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

//! `GeoJSON` sources, both discovered in a directory and served from a watched directory.

use std::fs;

use approx::assert_abs_diff_eq;
use martin_e2e_tests::{Martin, WatchedDir, fixture};
use mlt_core::fast_mvt::MvtValue;
use rstest::rstest;
use serde_json::Value;

async fn martin_with_geojson_dir() -> Martin {
    Martin::builder()
        .arg("tests/fixtures/geojson")
        .start()
        .await
        .expect("failed to start martin")
}

/// Rounds the last digits of a web mercator round-trip away, since each platform's libm lands on its own.
fn round_coordinates(value: &mut Value) {
    match value {
        Value::Number(number) if number.is_f64() => {
            let coordinate = number.as_f64().expect("the number is a float");
            *number = serde_json::Number::from_f64((coordinate * 1e6).round() / 1e6)
                .expect("a rounded coordinate is not finite");
        }
        Value::Array(items) => items.iter_mut().for_each(round_coordinates),
        Value::Object(entries) => entries.values_mut().for_each(round_coordinates),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Web mercator round-trips the bounds, so their last digits are noise.
fn assert_bounds(tilejson: &Value, expected: [f64; 4]) {
    let bounds = tilejson["bounds"]
        .as_array()
        .expect("the tilejson has no bounds");
    let [west, south, east, north] =
        <&[Value; 4]>::try_from(bounds.as_slice()).expect("the bounds are not four numbers");
    let actual =
        [west, south, east, north].map(|bound| bound.as_f64().expect("a bound is not a number"));
    assert_abs_diff_eq!(actual[..], expected[..], epsilon = 1e-6);
}

#[tokio::test]
async fn every_geojson_file_becomes_a_source() {
    let mut martin = martin_with_geojson_dir().await;

    let catalog = martin.get("/catalog").await;
    assert_eq!(catalog.status(), 200);
    insta::assert_snapshot!(catalog.headers_snapshot(), @"
        content-encoding: br
        content-type: application/json
        etag: [ETAG]
        transfer-encoding: chunked
        vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
        ");
    insta::assert_json_snapshot!(catalog.json()["tiles"], @r#"
    {
      "bare_geometry": {
        "content_type": "application/x-protobuf"
      },
      "clip": {
        "content_type": "application/x-protobuf"
      },
      "feature_1": {
        "content_type": "application/x-protobuf"
      },
      "feature_collection_1": {
        "content_type": "application/x-protobuf"
      },
      "feature_collection_2": {
        "content_type": "application/x-protobuf"
      },
      "feature_collection_3": {
        "content_type": "application/x-protobuf"
      },
      "multi_geometries": {
        "content_type": "application/x-protobuf"
      },
      "properties": {
        "content_type": "application/x-protobuf"
      }
    }
    "#);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn the_saved_config_names_every_discovered_file() {
    let tmp = tempfile::tempdir().expect("failed to create a temp dir");
    let save_config = tmp.path().join("save_config.yaml");
    let mut martin = Martin::builder()
        .arg("--save-config")
        .arg(&save_config)
        .arg("tests/fixtures/geojson")
        .start()
        .await
        .expect("failed to start martin");

    let saved = fs::read_to_string(&save_config)
        .expect("martin did not write --save-config")
        .replace(std::path::MAIN_SEPARATOR, "/");
    // The discovered paths martin writes carry the OS path separator; normalized here so the
    // snapshot is the same on every platform.
    let saved = saved.replace(std::path::MAIN_SEPARATOR, "/");
    insta::assert_snapshot!(saved, @"
    listen_addresses: 127.0.0.1:0
    pmtiles:
      paths: tests/fixtures/geojson
    mbtiles: tests/fixtures/geojson
    geojson:
      paths: tests/fixtures/geojson
      sources:
        bare_geometry: tests/fixtures/geojson/bare_geometry.geojson
        clip: tests/fixtures/geojson/clip.geojson
        feature_1: tests/fixtures/geojson/feature_1.geojson
        feature_collection_1: tests/fixtures/geojson/feature_collection_1.geojson
        feature_collection_2: tests/fixtures/geojson/feature_collection_2.geojson
        feature_collection_3: tests/fixtures/geojson/feature_collection_3.json
        multi_geometries: tests/fixtures/geojson/multi_geometries.geojson
        properties: tests/fixtures/geojson/properties.geojson
    ");

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_tilejson_points_back_at_the_source() {
    let mut martin = martin_with_geojson_dir().await;

    let response = martin.get("/feature_collection_1").await;
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
    let mut tilejson = serde_json::from_str::<Value>(&martin.redact(&response.text()))
        .expect("response body is not valid json");
    round_coordinates(&mut tilejson);
    insta::assert_json_snapshot!(tilejson, @r#"
    {
      "bounds": [
        -100.0,
        -50.0,
        25.0,
        50.0
      ],
      "center": [
        -37.5,
        0.0,
        0
      ],
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/feature_collection_1/{z}/{x}/{y}"
      ],
      "vector_layers": [
        {
          "fields": {
            "id": "Number"
          },
          "id": "feature_collection_1"
        }
      ]
    }
    "#);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[rstest]
#[case::one_source("feature_collection_1")]
#[case::a_composite_of_two_sources("feature_collection_1,properties")]
#[tokio::test]
async fn a_tilejson_advertises_every_layer_the_tiles_carry(#[case] source_ids: &str) {
    let mut martin = martin_with_geojson_dir().await;

    let advertised = martin.get(&format!("/{source_ids}")).await.json()["vector_layers"]
        .as_array()
        .expect("the tilejson has no vector_layers")
        .iter()
        .map(|layer| {
            layer["id"]
                .as_str()
                .expect("a layer id is not a string")
                .to_owned()
        })
        .collect::<Vec<_>>();
    let served = martin
        .get(&format!("/{source_ids}/0/0/0"))
        .await
        .mvt()
        .layers
        .iter()
        .map(|layer| layer.name.clone())
        .collect::<Vec<_>>();
    assert_eq!(advertised, served);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[rstest]
#[case::a_feature_collection("feature_collection_1", [-100.0, -50.0, 25.0, 50.0])]
#[case::a_feature_collection_of_mixed_geometries("feature_collection_2", [13.404, 52.5195, 13.406, 52.5208])]
#[case::a_feature_collection_in_a_json_file("feature_collection_3", [-100.0, -50.0, 25.0, 50.0])]
#[case::a_bare_feature("feature_1", [13.405, 52.52, 13.405, 52.52])]
#[case::a_bare_geometry("bare_geometry", [10.0, 10.0, 20.0, 20.0])]
#[case::many_multi_geometries("multi_geometries", [-70.0, -30.0, 65.0, 65.0])]
#[case::a_point_at_null_island("properties", [0.0, 0.0, 0.0, 0.0])]
#[tokio::test]
async fn a_tilejson_bounds_the_features(#[case] source_id: &str, #[case] bounds: [f64; 4]) {
    let mut martin = martin_with_geojson_dir().await;

    assert_bounds(&martin.get(&format!("/{source_id}")).await.json(), bounds);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[rstest]
#[case::a_feature_collection("feature_collection_1", 3)]
#[case::a_feature_collection_of_mixed_geometries("feature_collection_2", 1)]
#[case::a_feature_collection_in_a_json_file("feature_collection_3", 3)]
#[case::a_bare_feature("feature_1", 1)]
#[case::a_bare_geometry("bare_geometry", 1)]
#[case::many_multi_geometries("multi_geometries", 5)]
#[tokio::test]
async fn a_source_serves_one_layer_named_after_itself(
    #[case] source_id: &str,
    #[case] features: usize,
) {
    let mut martin = martin_with_geojson_dir().await;

    let tile = martin.get(&format!("/{source_id}/0/0/0")).await;
    assert_eq!(tile.status(), 200);
    assert_eq!(tile.header("content-type"), Some("application/x-protobuf"));
    let layers = tile.mvt().layers;
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].name, source_id);
    assert_eq!(layers[0].features.len(), features);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_tile_is_served_gzipped_with_an_etag() {
    let mut martin = martin_with_geojson_dir().await;

    let tile = martin.get("/feature_collection_1/0/0/0").await;
    assert_eq!(tile.status(), 200);
    insta::assert_snapshot!(tile.headers_snapshot(), @r#"
    content-encoding: gzip
    content-length: 143
    content-type: application/x-protobuf
    etag: "Wtlvu7ZHlUF7ibfKmKKoag"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn property_types_survive_the_round_trip() {
    let mut martin = martin_with_geojson_dir().await;

    insta::assert_snapshot!(martin.get("/properties/0/0/0").await.mvt_dump(), @r#"
    layer: 0
      name: properties
      version: 2
      extent: 4096
      feature: 0
        id: (none)
        geometry: POINT(2048,2048)
        properties:
          prop_array = "[1,2,3]"
          prop_bool_false = false (bool)
          prop_bool_true = true (bool)
          prop_float = 3.5 (double)
          prop_int_negative = -42 (int)
          prop_object = "{\"nested\":\"value\"}"
          prop_string = "hello"
          prop_uint_large = 18446744073709551615 (uint)
    "#);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[rstest]
#[case::the_whole_world("clip/0/0/0", &[1, 2])]
#[case::the_north_west_quadrant("clip/1/0/0", &[1])]
#[case::the_north_east_quadrant("clip/1/1/0", &[1, 2])]
#[case::the_south_west_quadrant("clip/1/0/1", &[1])]
#[case::the_south_east_quadrant("clip/1/1/1", &[1])]
#[tokio::test]
async fn clipping_keeps_the_features_a_tile_overlaps(#[case] path: &str, #[case] ids: &[i64]) {
    let mut martin = martin_with_geojson_dir().await;

    let tile = martin.get(&format!("/{path}")).await;
    assert_eq!(tile.status(), 200);
    let layers = tile.mvt().layers;
    let present = layers[0]
        .features
        .iter()
        .map(|feature| match feature.properties.as_slice() {
            [(key, MvtValue::Int(id)), ..] if key == "id" => *id,
            properties => panic!("expected an id property, got {properties:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(present, ids);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_tile_without_features_is_no_content() {
    let mut martin = martin_with_geojson_dir().await;

    let tile = martin.get("/feature_1/1/0/1").await;
    assert_eq!(tile.status(), 204);
    assert!(tile.body().is_empty(), "a 204 must not carry a body");

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

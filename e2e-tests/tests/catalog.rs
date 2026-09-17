//! The `/catalog` document every source kind contributes to.

use martin_e2e_tests::{Martin, fixture};
use pretty_assertions::assert_eq;
use serde_json::Value;

async fn martin_with_every_source_kind() -> Martin {
    Martin::builder()
        .arg("tests/fixtures/geojson")
        .arg("--sprite")
        .arg(fixture("sprites/src1"))
        .arg("--sprite")
        .arg(fixture("sprites/src2"))
        .arg("--font")
        .arg(fixture("fonts"))
        .arg("--style")
        .arg(fixture("styles/maplibre_demo.json"))
        .arg("--style")
        .arg(fixture("styles/src2"))
        .start()
        .await
        .expect("failed to start martin")
}

/// Normalizes path separators inside every string leaf of a decoded JSON value,
/// so the snapshot below reads the same on windows as it does elsewhere.
fn normalize_paths(value: &mut Value) {
    match value {
        Value::String(s) => *s = s.replace(std::path::MAIN_SEPARATOR, "/"),
        Value::Array(items) => items.iter_mut().for_each(normalize_paths),
        Value::Object(map) => {
            map.values_mut().for_each(normalize_paths);
            map.sort_keys();
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Two servers over the same sources answer a byte-identical catalog under the same etag.
#[tokio::test]
async fn two_servers_over_the_same_sources_answer_the_same_bytes() {
    let mut first = martin_with_every_source_kind().await;
    let mut second = martin_with_every_source_kind().await;

    let first_catalog = first.get("/catalog").await;
    let second_catalog = second.get("/catalog").await;
    assert_eq!(first_catalog.status(), 200);
    assert_eq!(second_catalog.status(), 200);
    assert_eq!(first_catalog.text(), second_catalog.text());
    assert_eq!(first_catalog.header("etag"), second_catalog.header("etag"));

    let mut catalog = first_catalog.json();
    normalize_paths(&mut catalog);
    insta::assert_json_snapshot!(catalog, {".settings" => "[cfg dependent]"}, @r#"
    {
      "fonts": {
        "Overpass Mono Light": {
          "end": 128276,
          "family": "Overpass Mono",
          "format": "otf",
          "glyphs": 988,
          "start": 0,
          "style": "Light"
        },
        "Overpass Mono Regular": {
          "end": 128276,
          "family": "Overpass Mono",
          "format": "ttf",
          "glyphs": 988,
          "start": 0,
          "style": "Regular"
        }
      },
      "settings": "[cfg dependent]",
      "sprites": {
        "src1": {
          "images": [
            "another_bicycle",
            "bear",
            "sub/circle"
          ]
        },
        "src2": {
          "images": [
            "bicycle"
          ]
        }
      },
      "styles": {
        "maplibre_demo": {
          "path": "tests/fixtures/styles/maplibre_demo.json"
        },
        "maptiler_basic": {
          "path": "tests/fixtures/styles/src2/maptiler_basic.json"
        },
        "osm-liberty-lite": {
          "path": "tests/fixtures/styles/src2/osm-liberty-lite.json"
        }
      },
      "tiles": {
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
    }
    "#);

    for martin in [&mut first, &mut second] {
        martin.stop().await;
        martin.assert_startup_warnings();
    }
}

#[tokio::test]
async fn the_catalog_answers_conditional_requests() {
    let mut martin = martin_with_every_source_kind().await;

    let first = martin.get("/catalog").await;
    assert_eq!(first.status(), 200);
    let etag = first
        .header("etag")
        .expect("the catalog must carry an etag")
        .to_owned();

    let cached = martin
        .get_with_headers("/catalog", &[("if-none-match", &etag)])
        .await;
    assert_eq!(cached.status(), 304);
    assert!(cached.body().is_empty());

    let stale = martin
        .get_with_headers(
            "/catalog",
            &[("if-none-match", r#"W/"0-0000000000000000000000""#)],
        )
        .await;
    assert_eq!(stale.status(), 200);
    assert_eq!(stale.body(), first.body());

    martin.stop().await;
    martin.assert_startup_warnings();
}

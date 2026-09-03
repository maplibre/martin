//! `MapLibre` style documents served from files and directories.

use std::fs;

use martin_e2e_tests::{Martin, TestResponse, fixture};
use rstest::rstest;
use serde_json::Value;

async fn martin_with_styles() -> Martin {
    Martin::builder()
        .arg("--style")
        .arg(fixture("styles/maplibre_demo.json"))
        .arg("--style")
        .arg(fixture("styles/src2"))
        .arg("--style")
        .arg(fixture("styles/relative_urls.json"))
        .arg("--style")
        .arg(fixture("styles/composite_base.json"))
        .arg("--style")
        .arg(fixture("styles/composite_overlay.json"))
        .start()
        .await
        .expect("failed to start martin")
}

fn fixture_json(relative: &str) -> Value {
    let text = fs::read_to_string(fixture(relative)).expect("failed to read the fixture");
    serde_json::from_str(&text).expect("the fixture is not valid json")
}

fn redacted_json(martin: &Martin, response: &TestResponse) -> Value {
    serde_json::from_str(&martin.redact(&response.text())).expect("response body is not valid json")
}

/// Normalizes path separators inside every string leaf of a decoded JSON value.
///
/// Must run after parsing, not on the raw response text: JSON escapes `\` as `\\`, so replacing
/// the separator beforehand doubles it up into `//` instead of collapsing it to `/`.
fn normalize_paths(value: &mut Value) {
    match value {
        Value::String(s) => *s = s.replace(std::path::MAIN_SEPARATOR, "/"),
        Value::Array(items) => items.iter_mut().for_each(normalize_paths),
        Value::Object(map) => map.values_mut().for_each(normalize_paths),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[tokio::test]
async fn styles_are_discovered_from_files_and_directories() {
    let mut martin = martin_with_styles().await;

    let mut styles = martin.get("/catalog").await.json()["styles"].clone();
    normalize_paths(&mut styles);
    insta::assert_json_snapshot!(styles, @r#"
    {
      "composite_base": {
        "path": "tests/fixtures/styles/composite_base.json"
      },
      "composite_overlay": {
        "path": "tests/fixtures/styles/composite_overlay.json"
      },
      "maplibre_demo": {
        "path": "tests/fixtures/styles/maplibre_demo.json"
      },
      "maptiler_basic": {
        "path": "tests/fixtures/styles/src2/maptiler_basic.json"
      },
      "osm-liberty-lite": {
        "path": "tests/fixtures/styles/src2/osm-liberty-lite.json"
      },
      "relative_urls": {
        "path": "tests/fixtures/styles/relative_urls.json"
      }
    }
    "#);

    martin.stop().await;
}

#[tokio::test]
async fn a_style_is_served_as_json() {
    let mut martin = martin_with_styles().await;

    let response = martin.get("/style/maplibre_demo").await;
    assert_eq!(response.status(), 200);
    insta::assert_snapshot!(response.headers_snapshot(), @r#"
    content-encoding: br
    content-type: application/json
    etag: W/"1062-03lipI2ul-91qQlCIStBcA=="
    transfer-encoding: chunked
    vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    assert_eq!(response.json(), fixture_json("styles/maplibre_demo.json"));

    martin.stop().await;
}

#[rstest]
#[case::a_file_source("maplibre_demo", "styles/maplibre_demo.json")]
#[case::a_directory_source("maptiler_basic", "styles/src2/maptiler_basic.json")]
#[case::a_hyphenated_id("osm-liberty-lite", "styles/src2/osm-liberty-lite.json")]
#[tokio::test]
async fn the_json_suffix_is_optional(#[case] style_id: &str, #[case] fixture_path: &str) {
    let mut martin = martin_with_styles().await;

    let bare = martin.get(&format!("/style/{style_id}")).await;
    let suffixed = martin.get(&format!("/style/{style_id}.json")).await;
    assert_eq!(bare.status(), 200);
    assert_eq!(suffixed.status(), 200);
    assert_eq!(bare.json(), fixture_json(fixture_path));
    assert_eq!(bare.body(), suffixed.body());

    martin.stop().await;
}

#[tokio::test]
async fn styles_can_be_merged_in_request_order() {
    let mut martin = martin_with_styles().await;

    let response = martin
        .get_with_headers(
            "/style/composite_base,composite_overlay",
            &[("Host", "example.com")],
        )
        .await;
    let suffixed = martin
        .get_with_headers(
            "/style/composite_base,composite_overlay.json",
            &[("Host", "example.com")],
        )
        .await;
    assert_eq!(response.status(), 200);
    assert_eq!(suffixed.status(), 200);
    assert_eq!(response.body(), suffixed.body());
    insta::assert_json_snapshot!(response.json(), @r##"
    {
      "font-faces": {
        "Base Font": "https://fonts.example/base.ttf",
        "Overlay Font": "https://fonts.example/overlay.ttf"
      },
      "glyphs": "http://example.com/font/{fontstack}/{range}",
      "layers": [
        {
          "id": "base-layer",
          "source": "canonical",
          "source-layer": "roads",
          "type": "line"
        },
        {
          "id": "overlay-line",
          "source": "canonical",
          "source-layer": "roads",
          "type": "line"
        },
        {
          "id": "overlay-points",
          "layout": {
            "text-field": "label",
            "text-font": [
              "Overlay Font"
            ]
          },
          "paint": {
            "text-opacity": [
              "global-state",
              "overlay-opacity"
            ]
          },
          "source": "points",
          "type": "symbol"
        }
      ],
      "metadata": {
        "from": "base"
      },
      "name": "Composite base",
      "sources": {
        "canonical": {
          "type": "vector",
          "url": "http://example.com/shared_source"
        },
        "points": {
          "data": {
            "features": [],
            "type": "FeatureCollection"
          },
          "type": "geojson"
        }
      },
      "sprite": "http://example.com/sprite/src1,src2",
      "state": {
        "base-visible": {
          "default": true
        },
        "overlay-opacity": {
          "default": 0.5
        }
      },
      "version": 8
    }
    "##);

    martin.stop().await;
    martin.assert_log_clean();
}

#[tokio::test]
async fn composite_style_validation_errors_are_bad_requests() {
    let mut martin = martin_with_styles().await;

    let empty = martin.get("/style/composite_base,,composite_overlay").await;
    assert_eq!(empty.status(), 400);
    assert_eq!(empty.text(), "Style ids must not be empty");

    let conflict = martin.get("/style/maplibre_demo,relative_urls").await;
    assert_eq!(conflict.status(), 400);
    assert_eq!(
        conflict.text(),
        "Cannot merge styles \"maplibre_demo\" and \"relative_urls\": glyph URLs are different"
    );

    let missing = martin.get("/style/composite_base,nope").await;
    assert_eq!(missing.status(), 404);

    martin.stop().await;
    martin.assert_log_clean();
}

#[tokio::test]
async fn relative_urls_expand_against_the_listen_address() {
    let mut martin = martin_with_styles().await;

    let response = martin.get("/style/relative_urls").await;
    assert_eq!(response.status(), 200);
    insta::assert_json_snapshot!(redacted_json(&martin, &response), @r##"
    {
      "glyphs": "http://[ADDR]/font/{fontstack}/{range}",
      "layers": [
        {
          "id": "background",
          "paint": {
            "background-color": "#000"
          },
          "type": "background"
        }
      ],
      "name": "Relative URLs Test",
      "sources": {
        "from_tiles": {
          "tiles": [
            "http://[ADDR]/points1/{z}/{x}/{y}"
          ],
          "type": "vector"
        },
        "from_url": {
          "type": "vector",
          "url": "http://[ADDR]/table_source"
        }
      },
      "sprite": "http://[ADDR]/sprite/src1",
      "version": 8
    }
    "##);

    martin.stop().await;
}

#[tokio::test]
async fn relative_urls_expand_against_the_host_header() {
    let mut martin = martin_with_styles().await;

    let response = martin
        .get_with_headers("/style/relative_urls", &[("Host", "example.com")])
        .await;
    assert_eq!(response.status(), 200);
    insta::assert_json_snapshot!(redacted_json(&martin, &response), @r##"
    {
      "glyphs": "http://example.com/font/{fontstack}/{range}",
      "layers": [
        {
          "id": "background",
          "paint": {
            "background-color": "#000"
          },
          "type": "background"
        }
      ],
      "name": "Relative URLs Test",
      "sources": {
        "from_tiles": {
          "tiles": [
            "http://example.com/points1/{z}/{x}/{y}"
          ],
          "type": "vector"
        },
        "from_url": {
          "type": "vector",
          "url": "http://example.com/table_source"
        }
      },
      "sprite": "http://example.com/sprite/src1",
      "version": 8
    }
    "##);

    martin.stop().await;
}

#[tokio::test]
async fn relative_urls_expand_against_a_host_with_a_port() {
    let mut martin = martin_with_styles().await;

    let response = martin
        .get_with_headers("/style/relative_urls", &[("Host", "example.com:6000")])
        .await;
    assert_eq!(response.status(), 200);
    insta::assert_json_snapshot!(redacted_json(&martin, &response), @r##"
    {
      "glyphs": "http://example.com:6000/font/{fontstack}/{range}",
      "layers": [
        {
          "id": "background",
          "paint": {
            "background-color": "#000"
          },
          "type": "background"
        }
      ],
      "name": "Relative URLs Test",
      "sources": {
        "from_tiles": {
          "tiles": [
            "http://example.com:6000/points1/{z}/{x}/{y}"
          ],
          "type": "vector"
        },
        "from_url": {
          "type": "vector",
          "url": "http://example.com:6000/table_source"
        }
      },
      "sprite": "http://example.com:6000/sprite/src1",
      "version": 8
    }
    "##);

    martin.stop().await;
}

#[tokio::test]
async fn relative_urls_expand_against_a_forwarded_host() {
    let mut martin = martin_with_styles().await;

    let response = martin
        .get_with_headers(
            "/style/relative_urls",
            &[("X-Forwarded-Host", "tiles.example.com")],
        )
        .await;
    assert_eq!(response.status(), 200);
    insta::assert_json_snapshot!(redacted_json(&martin, &response), @r##"
    {
      "glyphs": "http://tiles.example.com/font/{fontstack}/{range}",
      "layers": [
        {
          "id": "background",
          "paint": {
            "background-color": "#000"
          },
          "type": "background"
        }
      ],
      "name": "Relative URLs Test",
      "sources": {
        "from_tiles": {
          "tiles": [
            "http://tiles.example.com/points1/{z}/{x}/{y}"
          ],
          "type": "vector"
        },
        "from_url": {
          "type": "vector",
          "url": "http://tiles.example.com/table_source"
        }
      },
      "sprite": "http://tiles.example.com/sprite/src1",
      "version": 8
    }
    "##);

    martin.stop().await;
}

#[tokio::test]
async fn relative_urls_expand_against_a_forwarded_prefix() {
    let mut martin = martin_with_styles().await;

    let response = martin
        .get_with_headers("/style/relative_urls", &[("X-Forwarded-Prefix", "/tiles")])
        .await;
    assert_eq!(response.status(), 200);
    insta::assert_json_snapshot!(redacted_json(&martin, &response), @r##"
    {
      "glyphs": "http://[ADDR]/tiles/font/{fontstack}/{range}",
      "layers": [
        {
          "id": "background",
          "paint": {
            "background-color": "#000"
          },
          "type": "background"
        }
      ],
      "name": "Relative URLs Test",
      "sources": {
        "from_tiles": {
          "tiles": [
            "http://[ADDR]/tiles/points1/{z}/{x}/{y}"
          ],
          "type": "vector"
        },
        "from_url": {
          "type": "vector",
          "url": "http://[ADDR]/tiles/table_source"
        }
      },
      "sprite": "http://[ADDR]/tiles/sprite/src1",
      "version": 8
    }
    "##);

    martin.stop().await;
}

#[tokio::test]
async fn relative_urls_expand_against_a_forwarded_prefix_without_its_trailing_slash() {
    let mut martin = martin_with_styles().await;

    let response = martin
        .get_with_headers("/style/relative_urls", &[("X-Forwarded-Prefix", "/tiles/")])
        .await;
    assert_eq!(response.status(), 200);
    insta::assert_json_snapshot!(redacted_json(&martin, &response), @r##"
    {
      "glyphs": "http://[ADDR]/tiles/font/{fontstack}/{range}",
      "layers": [
        {
          "id": "background",
          "paint": {
            "background-color": "#000"
          },
          "type": "background"
        }
      ],
      "name": "Relative URLs Test",
      "sources": {
        "from_tiles": {
          "tiles": [
            "http://[ADDR]/tiles/points1/{z}/{x}/{y}"
          ],
          "type": "vector"
        },
        "from_url": {
          "type": "vector",
          "url": "http://[ADDR]/tiles/table_source"
        }
      },
      "sprite": "http://[ADDR]/tiles/sprite/src1",
      "version": 8
    }
    "##);

    martin.stop().await;
}

#[tokio::test]
async fn relative_urls_ignore_a_forwarded_for() {
    let mut martin = martin_with_styles().await;

    let response = martin
        .get_with_headers(
            "/style/relative_urls",
            &[("X-Forwarded-For", "forwarded-for.example.com")],
        )
        .await;
    assert_eq!(response.status(), 200);
    insta::assert_json_snapshot!(redacted_json(&martin, &response), @r##"
    {
      "glyphs": "http://[ADDR]/font/{fontstack}/{range}",
      "layers": [
        {
          "id": "background",
          "paint": {
            "background-color": "#000"
          },
          "type": "background"
        }
      ],
      "name": "Relative URLs Test",
      "sources": {
        "from_tiles": {
          "tiles": [
            "http://[ADDR]/points1/{z}/{x}/{y}"
          ],
          "type": "vector"
        },
        "from_url": {
          "type": "vector",
          "url": "http://[ADDR]/table_source"
        }
      },
      "sprite": "http://[ADDR]/sprite/src1",
      "version": 8
    }
    "##);

    martin.stop().await;
}

#[tokio::test]
async fn relative_urls_ignore_a_rewritten_url() {
    let mut martin = martin_with_styles().await;

    let response = martin
        .get_with_headers(
            "/style/relative_urls",
            &[("X-Rewrite-URL", "/footiles/style/relative_urls")],
        )
        .await;
    assert_eq!(response.status(), 200);
    insta::assert_json_snapshot!(redacted_json(&martin, &response), @r##"
    {
      "glyphs": "http://[ADDR]/font/{fontstack}/{range}",
      "layers": [
        {
          "id": "background",
          "paint": {
            "background-color": "#000"
          },
          "type": "background"
        }
      ],
      "name": "Relative URLs Test",
      "sources": {
        "from_tiles": {
          "tiles": [
            "http://[ADDR]/points1/{z}/{x}/{y}"
          ],
          "type": "vector"
        },
        "from_url": {
          "type": "vector",
          "url": "http://[ADDR]/table_source"
        }
      },
      "sprite": "http://[ADDR]/sprite/src1",
      "version": 8
    }
    "##);

    martin.stop().await;
}

#[rstest]
#[case::bare("/style/nope")]
#[case::with_a_json_suffix("/style/nope.json")]
#[case::a_directory("/style/src2")]
#[tokio::test]
async fn an_unknown_style_is_not_found(#[case] path: &str) {
    let mut martin = martin_with_styles().await;

    let response = martin.get(path).await;
    assert_eq!(response.status(), 404);
    assert_eq!(response.text(), "No such style exists");

    martin.stop().await;
}

#[tokio::test]
async fn the_plural_styles_path_redirects() {
    let mut martin = martin_with_styles().await;

    for response in [
        martin.get("/styles/maplibre_demo").await,
        martin.head("/styles/maplibre_demo").await,
    ] {
        assert_eq!(response.status(), 301);
        assert_eq!(response.header("location"), Some("/style/maplibre_demo"));
    }
    let target = martin.get("/style/maplibre_demo").await;
    assert_eq!(target.status(), 200);
    assert_eq!(target.json(), fixture_json("styles/maplibre_demo.json"));

    martin.stop().await;
}

#[tokio::test]
async fn a_collection_publishes_each_subdirectory_as_a_project() {
    let mut martin = Martin::builder()
        .config("styles:\n  collections: tests/fixtures/styles\n")
        .start()
        .await
        .expect("failed to start martin");

    let mut styles = martin.get("/catalog").await.json()["styles"].clone();
    normalize_paths(&mut styles);
    insta::assert_json_snapshot!(styles, @r#"
    {
      "src2.maptiler_basic": {
        "path": "tests/fixtures/styles/src2/maptiler_basic.json"
      },
      "src2.osm-liberty-lite": {
        "path": "tests/fixtures/styles/src2/osm-liberty-lite.json"
      }
    }
    "#);
    assert_eq!(martin.get("/style/src2.maptiler_basic").await.status(), 200);

    martin.stop().await;
}

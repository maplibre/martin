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

#[tokio::test]
async fn styles_are_discovered_from_files_and_directories() {
    let mut martin = martin_with_styles().await;

    insta::assert_json_snapshot!(martin.get("/catalog").await.json()["styles"], @r#"
    {
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
}

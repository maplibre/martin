//! `MapLibre` style documents served from files and directories.

use std::fs;

use martin_integration_tests::{Martin, TestResponse, fixture};
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

fn urls_in(style: &Value) -> Vec<String> {
    [
        &style["glyphs"],
        &style["sprite"],
        &style["sources"]["from_tiles"]["tiles"][0],
        &style["sources"]["from_url"]["url"],
    ]
    .into_iter()
    .map(|url| url.as_str().expect("the style is missing a url").to_owned())
    .collect()
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
#[case::a_file_source("maplibre_demo")]
#[case::a_directory_source("maptiler_basic")]
#[case::a_hyphenated_id("osm-liberty-lite")]
#[tokio::test]
async fn the_json_suffix_is_optional(#[case] style_id: &str) {
    let mut martin = martin_with_styles().await;

    let bare = martin.get(&format!("/style/{style_id}")).await;
    let suffixed = martin.get(&format!("/style/{style_id}.json")).await;
    assert_eq!(bare.status(), 200);
    assert_eq!(suffixed.status(), 200);
    assert_eq!(bare.body(), suffixed.body());

    martin.stop().await;
    martin.assert_log_clean();
}

#[tokio::test]
async fn relative_urls_expand_against_the_requested_server() {
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

#[rstest]
#[case::the_listen_address(&[], "http://[ADDR]")]
#[case::the_host_header(&[("Host", "example.com")], "http://example.com")]
#[case::a_host_with_a_port(&[("Host", "example.com:6000")], "http://example.com:6000")]
#[case::a_forwarded_host(&[("X-Forwarded-Host", "tiles.example.com")], "http://tiles.example.com")]
#[case::a_forwarded_prefix(&[("X-Forwarded-Prefix", "/tiles")], "http://[ADDR]/tiles")]
#[case::a_forwarded_prefix_without_its_trailing_slash(&[("X-Forwarded-Prefix", "/tiles/")], "http://[ADDR]/tiles")]
#[tokio::test]
async fn the_expansion_base_follows_the_request(
    #[case] headers: &[(&str, &str)],
    #[case] base: &str,
) {
    let mut martin = martin_with_styles().await;

    let response = martin
        .get_with_headers("/style/relative_urls", headers)
        .await;
    assert_eq!(response.status(), 200);
    for url in urls_in(&redacted_json(&martin, &response)) {
        assert!(
            url.starts_with(&format!("{base}/")),
            "{url} is not under {base}"
        );
    }

    martin.stop().await;
    martin.assert_log_clean();
}

#[rstest]
#[case::a_forwarded_for("X-Forwarded-For", "forwarded-for.example.com")]
#[case::a_rewritten_url("X-Rewrite-URL", "/footiles/style/relative_urls")]
#[tokio::test]
async fn the_expansion_base_ignores(#[case] header: &str, #[case] value: &str) {
    let mut martin = martin_with_styles().await;

    let plain = martin.get("/style/relative_urls").await;
    let with_header = martin
        .get_with_headers("/style/relative_urls", &[(header, value)])
        .await;
    assert_eq!(with_header.status(), 200);
    assert_eq!(with_header.body(), plain.body());

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
    assert_eq!(martin.get("/style/maplibre_demo").await.status(), 200);

    martin.stop().await;
    martin.assert_log_clean();
}

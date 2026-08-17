//! Where the `tiles` URL of a `TileJSON` response gets its scheme, authority, path prefix and query string.

use martin_e2e_tests::Martin;
use rstest::rstest;

async fn martin_with_a_pmtiles_source() -> Martin {
    Martin::builder()
        .arg("tests/fixtures/pmtiles2")
        .start()
        .await
        .expect("failed to start martin")
}

async fn tiles_url(martin: &Martin, path: &str, headers: &[(&str, &str)]) -> String {
    let response = martin.get_with_headers(path, headers).await;
    assert_eq!(response.status(), 200);
    martin.redact(
        response.json()["tiles"][0]
            .as_str()
            .expect("the tilejson has no tiles url"),
    )
}

#[tokio::test]
async fn a_tilejson_carries_the_source_metadata_and_a_tiles_url() {
    let mut martin = martin_with_a_pmtiles_source().await;

    let response = martin.get("/webp2").await;
    assert_eq!(response.status(), 200);
    insta::with_settings!({filters => vec![(r"(?m)^etag: .*$", "etag: [ETAG]")]}, {
        insta::assert_snapshot!(response.headers_snapshot(), @r"
        content-encoding: br
        content-type: application/json
        etag: [ETAG]
        transfer-encoding: chunked
        vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
        ");
    });
    let tilejson = serde_json::from_str::<serde_json::Value>(&martin.redact(&response.text()))
        .expect("response body is not valid json");
    insta::assert_json_snapshot!(tilejson, @r#"
    {
      "basename": "ne2sr.mbtiles",
      "bounds": [
        -180.0,
        -85.05113,
        180.0,
        85.05113
      ],
      "center": [
        0.0,
        0.0,
        0
      ],
      "filesize": "21409792",
      "format": "webp",
      "id": "ne2sr",
      "maxzoom": 1,
      "minzoom": 0,
      "name": "ne2sr",
      "scheme": "tms",
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/webp2/{z}/{x}/{y}"
      ],
      "type": "baselayer",
      "vector_layers": []
    }
    "#);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_tilejson_answers_head_without_a_body() {
    let mut martin = martin_with_a_pmtiles_source().await;

    let response = martin.head("/webp2").await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert!(response.body().is_empty(), "HEAD must not return a body");

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[rstest]
#[case::no_proxy_headers(&[], "http://[ADDR]/webp2/{z}/{x}/{y}")]
#[case::a_host(&[("host", "example.com")], "http://example.com/webp2/{z}/{x}/{y}")]
#[case::a_host_with_a_port(&[("host", "example.com:6000")], "http://example.com:6000/webp2/{z}/{x}/{y}")]
#[case::an_x_forwarded_proto_of_https(&[("x-forwarded-proto", "https")], "https://[ADDR]/webp2/{z}/{x}/{y}")]
#[case::an_x_forwarded_proto_of_http(&[("x-forwarded-proto", "http")], "http://[ADDR]/webp2/{z}/{x}/{y}")]
#[case::an_x_forwarded_host(&[("x-forwarded-host", "tiles.example.com")], "http://tiles.example.com/webp2/{z}/{x}/{y}")]
#[case::a_forwarded_proto(&[("forwarded", "proto=https")], "https://[ADDR]/webp2/{z}/{x}/{y}")]
#[case::a_forwarded_host(&[("forwarded", "host=tiles.example.com")], "http://tiles.example.com/webp2/{z}/{x}/{y}")]
#[case::a_forwarded_proto_and_host(&[("forwarded", "proto=https;host=tiles.example.com")], "https://tiles.example.com/webp2/{z}/{x}/{y}")]
#[tokio::test]
async fn proxy_headers_set_the_scheme_and_authority(
    #[case] headers: &[(&str, &str)],
    #[case] expected: &str,
) {
    let mut martin = martin_with_a_pmtiles_source().await;

    assert_eq!(tiles_url(&martin, "/webp2", headers).await, expected);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn an_x_forwarded_for_does_not_change_the_url() {
    let mut martin = martin_with_a_pmtiles_source().await;

    assert_eq!(
        tiles_url(&martin, "/webp2", &[("x-forwarded-for", "192.168.1.100")]).await,
        "http://[ADDR]/webp2/{z}/{x}/{y}"
    );

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[rstest]
#[case::an_x_rewrite_url(&[("x-rewrite-url", "/footiles/webp2")], "http://[ADDR]/footiles/webp2/{z}/{x}/{y}")]
#[case::an_x_forwarded_prefix(&[("x-forwarded-prefix", "/tiles/webp2")], "http://[ADDR]/tiles/webp2/{z}/{x}/{y}")]
#[tokio::test]
async fn a_rewrite_header_replaces_the_path(
    #[case] headers: &[(&str, &str)],
    #[case] expected: &str,
) {
    let mut martin = martin_with_a_pmtiles_source().await;

    assert_eq!(tiles_url(&martin, "/webp2", headers).await, expected);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn x_rewrite_url_wins_over_x_forwarded_prefix() {
    let mut martin = martin_with_a_pmtiles_source().await;

    let headers = [
        ("x-rewrite-url", "/footiles/webp2"),
        ("x-forwarded-prefix", "/tiles/webp2"),
    ];
    assert_eq!(
        tiles_url(&martin, "/webp2", &headers).await,
        "http://[ADDR]/footiles/webp2/{z}/{x}/{y}"
    );

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn only_the_path_of_a_rewrite_header_is_used() {
    let mut martin = martin_with_a_pmtiles_source().await;

    let headers = [(
        "x-rewrite-url",
        "https://elsewhere.example.com/footiles/webp2?ignored=1",
    )];
    assert_eq!(
        tiles_url(&martin, "/webp2", &headers).await,
        "http://[ADDR]/footiles/webp2/{z}/{x}/{y}"
    );

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn the_request_query_string_is_carried_into_the_tiles_url() {
    let mut martin = martin_with_a_pmtiles_source().await;

    assert_eq!(
        tiles_url(&martin, "/webp2?foo=bar&token=martin", &[]).await,
        "http://[ADDR]/webp2/{z}/{x}/{y}?foo=bar&token=martin"
    );

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[rstest]
#[case::without_a_rewrite_header(&[])]
#[case::despite_an_x_rewrite_url(&[("x-rewrite-url", "/footiles/webp2")])]
#[case::despite_an_x_forwarded_prefix(&[("x-forwarded-prefix", "/tiles/webp2")])]
#[tokio::test]
async fn base_path_sets_the_path_prefix(#[case] headers: &[(&str, &str)]) {
    let mut martin = Martin::builder()
        .arg("--base-path")
        .arg("/bp")
        .arg("tests/fixtures/pmtiles2")
        .start()
        .await
        .expect("failed to start martin");

    assert_eq!(
        tiles_url(&martin, "/webp2", headers).await,
        "http://[ADDR]/bp/webp2/{z}/{x}/{y}"
    );

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[rstest]
#[case::without_a_rewrite_header(&[])]
#[case::despite_an_x_rewrite_url(&[("x-rewrite-url", "/footiles/webp2")])]
#[tokio::test]
async fn route_prefix_sets_the_path_prefix(#[case] headers: &[(&str, &str)]) {
    let mut martin = Martin::builder()
        .arg("--route-prefix")
        .arg("/foo")
        .arg("tests/fixtures/pmtiles2")
        .start()
        .await
        .expect("failed to start martin");

    assert_eq!(
        tiles_url(&martin, "/foo/webp2", headers).await,
        "http://[ADDR]/foo/webp2/{z}/{x}/{y}"
    );

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn base_path_wins_over_route_prefix() {
    let mut martin = Martin::builder()
        .arg("--base-path")
        .arg("/bp")
        .arg("--route-prefix")
        .arg("/foo")
        .arg("tests/fixtures/pmtiles2")
        .start()
        .await
        .expect("failed to start martin");

    assert_eq!(
        tiles_url(&martin, "/foo/webp2", &[]).await,
        "http://[ADDR]/bp/webp2/{z}/{x}/{y}"
    );

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

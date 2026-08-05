//! Prometheus metrics served at `/_/metrics`.

use martin_integration_tests::Martin;

async fn martin_with_a_pmtiles_source() -> Martin {
    Martin::builder()
        .arg("tests/fixtures/pmtiles/png.pmtiles")
        .start()
        .await
        .expect("failed to start martin")
}

/// How long a request took decides which duration buckets it lands in, so only the bucket a
/// request is guaranteed to reach (`+Inf`), the totals and the counts are asserted verbatim.
fn scrape_filters() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            r#"(?m)^(martin_http_requests_duration_seconds_bucket\{.*le="[0-9.]+"\}) \d+$"#,
            "$1 [COUNT]",
        ),
        (
            r"(?m)^(martin_http_requests_duration_seconds_sum\{.*\}) \S+$",
            "$1 [SECONDS]",
        ),
    ]
}

#[tokio::test]
async fn a_scrape_reports_every_family_with_a_series_per_route_pattern() {
    let mut martin = martin_with_a_pmtiles_source().await;

    assert_eq!(martin.get("/catalog").await.status(), 200);
    assert_eq!(martin.get("/png").await.status(), 200);
    for _ in 0..3 {
        assert_eq!(martin.get("/png/0/0/0").await.status(), 200);
    }
    assert_eq!(martin.get("/no_such_source/0/0/0").await.status(), 404);

    let scrape = martin.get("/_/metrics").await.text();
    insta::with_settings!({filters => scrape_filters()}, {
        insta::assert_snapshot!("every_family", scrape);
    });

    martin.stop().await;
    martin.assert_log_contains(r#"ERROR error="Source no_such_source does not exist""#);
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_path_matching_no_route_is_counted_under_a_single_masked_label() {
    let mut martin = martin_with_a_pmtiles_source().await;

    assert_eq!(martin.get("/one/path/that/is/no/route").await.status(), 404);
    assert_eq!(
        martin.get("/another/path/that/is/no/route").await.status(),
        404
    );

    let scrape = martin.get("/_/metrics").await.text();
    insta::with_settings!({filters => scrape_filters()}, {
        insta::assert_snapshot!("masked_label", scrape);
    });

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_repeated_tile_request_is_counted_as_a_tile_cache_hit() {
    let mut martin = martin_with_a_pmtiles_source().await;

    assert_eq!(martin.get("/png/0/0/0").await.status(), 200);
    assert_eq!(martin.get("/png/0/0/0").await.status(), 200);

    let scrape = martin.get("/_/metrics").await.text();
    insta::with_settings!({filters => scrape_filters()}, {
        insta::assert_snapshot!("tile_cache_hit", scrape);
    });

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn metrics_are_served_as_prometheus_text_without_compressing_them() {
    let mut martin = martin_with_a_pmtiles_source().await;

    let metrics = martin.get("/_/metrics").await;
    assert_eq!(metrics.status(), 200);
    insta::with_settings!({filters => vec![(r"(?m)^content-length: \d+$", "content-length: [LENGTH]")]}, {
        insta::assert_snapshot!(metrics.headers_snapshot(), @r"
        content-length: [LENGTH]
        content-type: text/plain; version=0.0.4; charset=utf-8
        vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
        ");
    });

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

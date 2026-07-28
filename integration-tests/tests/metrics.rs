//! Prometheus metrics served at `/_/metrics`.

use martin_integration_tests::Martin;

#[tokio::test]
async fn a_repeated_tile_request_is_counted_as_a_tile_cache_hit() {
    let mut martin = Martin::builder()
        .arg("tests/fixtures/pmtiles/png.pmtiles")
        .start()
        .await
        .expect("failed to start martin");

    assert_eq!(martin.get("/png/0/0/0").await.status(), 200);
    assert_eq!(martin.get("/png/0/0/0").await.status(), 200);

    let metrics = martin.get("/_/metrics").await;
    assert_eq!(metrics.status(), 200);
    let cache_counters = metrics
        .text()
        .lines()
        .filter(|line| line.starts_with("martin_tile_cache_requests_total{"))
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(cache_counters, @r#"
    martin_tile_cache_requests_total{cache="tile",result="hit",zoom="0"} 1
    martin_tile_cache_requests_total{cache="tile",result="miss",zoom="0"} 1
    "#);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

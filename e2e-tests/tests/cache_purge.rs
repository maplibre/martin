//! The opt-in `DELETE /cache/{source_id}` route.

use martin_e2e_tests::Martin;

const ENABLED: &str = "
purge_endpoint: true
pmtiles:
  sources:
    pmt: tests/fixtures/pmtiles/png.pmtiles
";

const DISABLED: &str = "
pmtiles:
  sources:
    pmt: tests/fixtures/pmtiles/png.pmtiles
";

#[tokio::test]
async fn purging_a_source_drops_its_cached_tiles() {
    let mut martin = Martin::builder()
        .config(ENABLED)
        .start()
        .await
        .expect("failed to start martin");

    assert_eq!(martin.get("/pmt/0/0/0").await.status(), 200);
    let purged = martin.delete("/cache/pmt").await;
    assert_eq!(purged.status(), 204);
    assert!(purged.body().is_empty());
    assert_eq!(martin.get("/pmt/0/0/0").await.status(), 200);

    assert_eq!(martin.delete("/cache/nope").await.status(), 404);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_contains("Invalidated tile cache for source: pmt");
}

#[tokio::test]
async fn the_route_is_absent_unless_enabled() {
    let mut martin = Martin::builder()
        .config(DISABLED)
        .start()
        .await
        .expect("failed to start martin");

    assert_eq!(martin.delete("/cache/pmt").await.status(), 404);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

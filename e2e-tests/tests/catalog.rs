//! Conditional requests against `/catalog`.

use martin_e2e_tests::Martin;

async fn martin_with_a_pmtiles_source() -> Martin {
    Martin::builder()
        .arg("tests/fixtures/pmtiles2")
        .start()
        .await
        .expect("failed to start martin")
}

#[tokio::test]
async fn the_catalog_answers_conditional_requests() {
    let mut martin = martin_with_a_pmtiles_source().await;

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
    martin.assert_log_clean();
}

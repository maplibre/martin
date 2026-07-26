//! Routing behaviour shared by every tile source: redirects from a format suffix and from the `/tiles/` prefix.

use martin_integration_tests::Martin;
use rstest::rstest;

async fn martin_with_a_pmtiles_source() -> Martin {
    Martin::builder()
        .arg("tests/fixtures/pmtiles2")
        .start()
        .await
        .expect("failed to start martin")
}

#[rstest]
#[case::pbf("/webp2/0/0/0.pbf")]
#[case::mvt("/webp2/0/0/0.mvt")]
#[case::mlt("/webp2/0/0/0.mlt")]
#[case::png("/webp2/0/0/0.png")]
#[tokio::test]
async fn any_tile_format_suffix_redirects_to_the_extensionless_path(#[case] path: &str) {
    let mut martin = martin_with_a_pmtiles_source().await;

    for response in [martin.get(path).await, martin.head(path).await] {
        assert_eq!(response.status(), 301);
        assert_eq!(response.header("location"), Some("/webp2/0/0/0"));
    }
    assert_eq!(martin.get("/webp2/0/0/0").await.status(), 200);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn the_tiles_prefix_redirects_to_the_bare_source_path() {
    let mut martin = martin_with_a_pmtiles_source().await;

    for response in [
        martin.get("/tiles/webp2/0/0/0").await,
        martin.head("/tiles/webp2/0/0/0").await,
    ] {
        assert_eq!(response.status(), 301);
        assert_eq!(response.header("location"), Some("/webp2/0/0/0"));
    }
    assert_eq!(martin.get("/webp2/0/0/0").await.status(), 200);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[rstest]
#[case::suffix("/webp2/0/0/0.pbf?test=123")]
#[case::prefix("/tiles/webp2/0/0/0?test=123")]
#[tokio::test]
async fn a_redirect_keeps_the_query_string(#[case] path: &str) {
    let mut martin = martin_with_a_pmtiles_source().await;

    let response = martin.get(path).await;
    assert_eq!(response.status(), 301);
    assert_eq!(response.header("location"), Some("/webp2/0/0/0?test=123"));

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[rstest]
#[case::suffix("/foo/webp2/0/0/0.pbf")]
#[case::prefix("/foo/tiles/webp2/0/0/0")]
#[tokio::test]
async fn a_redirect_points_below_the_route_prefix(#[case] path: &str) {
    let mut martin = Martin::builder()
        .arg("--route-prefix")
        .arg("/foo")
        .arg("tests/fixtures/pmtiles2")
        .start()
        .await
        .expect("failed to start martin");

    let response = martin.get(path).await;
    assert_eq!(response.status(), 301);
    assert_eq!(response.header("location"), Some("/foo/webp2/0/0/0"));
    assert_eq!(martin.get("/foo/webp2/0/0/0").await.status(), 200);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[rstest]
#[case::suffix("/nosuch/0/0/0.pbf")]
#[case::prefix("/tiles/nosuch/0/0/0")]
#[tokio::test]
async fn a_redirect_is_issued_before_the_source_is_resolved(#[case] path: &str) {
    let mut martin = martin_with_a_pmtiles_source().await;

    let response = martin.get(path).await;
    assert_eq!(response.status(), 301);
    assert_eq!(response.header("location"), Some("/nosuch/0/0/0"));
    assert_eq!(martin.get("/nosuch/0/0/0").await.status(), 404);

    martin.stop().await;
    martin.assert_log_contains(r#"ERROR error="Source nosuch does not exist""#);
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

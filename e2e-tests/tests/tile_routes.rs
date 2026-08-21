//! Routing behaviour shared by every tile source: redirects from a format suffix and from the `/tiles/` prefix, `Accept` header negotiation, and the zoom range a source covers.

use martin_e2e_tests::Martin;
use rstest::rstest;

async fn martin_with_a_pmtiles_source() -> Martin {
    Martin::builder()
        .arg("tests/fixtures/pmtiles2")
        .start()
        .await
        .expect("failed to start martin")
}

async fn martin_with_a_vector_and_a_raster_source() -> Martin {
    Martin::builder()
        .arg("tests/fixtures/geojson/feature_1.geojson")
        .arg("tests/fixtures/pmtiles/png.pmtiles")
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

#[rstest]
#[case::the_source_format("application/x-protobuf")]
#[case::the_mapbox_alias_of_the_source_format("application/vnd.mapbox-vector-tile")]
#[case::anything("*/*")]
#[case::a_list_containing_the_source_format("image/png, application/x-protobuf")]
#[case::the_least_preferred_of_a_list("image/png;q=0.9, application/x-protobuf;q=0.1")]
#[tokio::test]
async fn a_vector_source_serves_mvt_when_the_accept_header_allows_it(#[case] accept: &str) {
    let mut martin = martin_with_a_vector_and_a_raster_source().await;

    let tile = martin
        .get_with_headers("/feature_1/0/0/0", &[("accept", accept)])
        .await;
    assert_eq!(tile.status(), 200);
    assert_eq!(tile.header("content-type"), Some("application/x-protobuf"));
    let layers = tile.mvt().layers;
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].name, "feature_1");
    assert_eq!(layers[0].features.len(), 1);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[rstest]
#[case::the_maplibre_tile_type("application/vnd.maplibre-tile")]
#[case::its_vector_tile_alias("application/vnd.maplibre-vector-tile")]
#[tokio::test]
async fn a_vector_source_transcodes_to_mlt_for_an_mlt_accept_header(#[case] accept: &str) {
    let mut martin = martin_with_a_vector_and_a_raster_source().await;

    let tile = martin
        .get_with_headers("/feature_1/0/0/0", &[("accept", accept)])
        .await;
    assert_eq!(tile.status(), 200);
    assert_eq!(
        tile.header("content-type"),
        Some("application/vnd.maplibre-tile")
    );
    let layers = tile.mlt();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].name(), "feature_1");
    assert_eq!(layers[0].feature_count(), 1);
    assert_eq!(layers[0].property_names(), ["id", "name"]);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[rstest]
#[case::png("image/png")]
#[case::every_image_format("image/*")]
#[case::json("application/json")]
#[tokio::test]
async fn a_vector_source_rejects_an_accept_header_of_other_formats(#[case] accept: &str) {
    let mut martin = martin_with_a_vector_and_a_raster_source().await;

    let response = martin
        .get_with_headers("/feature_1/0/0/0", &[("accept", accept)])
        .await;
    assert_eq!(response.status(), 406);
    assert_eq!(
        response.text(),
        "Source produces application/x-protobuf, which does not match the Accept header"
    );
    let head = martin
        .head_with_headers("/feature_1/0/0/0", &[("accept", accept)])
        .await;
    assert_eq!(head.status(), 406);

    martin.stop().await;
    martin.assert_log_contains(
        r#"ERROR error="Source produces application/x-protobuf, which does not match the Accept header""#,
    );
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[rstest]
#[case::the_source_format("image/png")]
#[case::every_image_format("image/*")]
#[case::anything("*/*")]
#[tokio::test]
async fn a_raster_source_serves_png_when_the_accept_header_allows_it(#[case] accept: &str) {
    let mut martin = martin_with_a_vector_and_a_raster_source().await;

    let tile = martin
        .get_with_headers("/png/0/0/0", &[("accept", accept)])
        .await;
    assert_eq!(tile.status(), 200);
    assert_eq!(tile.header("content-type"), Some("image/png"));
    assert_eq!(&tile.body()[..8], b"\x89PNG\r\n\x1a\n");

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[rstest]
#[case::mvt("application/x-protobuf")]
#[case::mlt("application/vnd.maplibre-tile")]
#[tokio::test]
async fn a_raster_source_is_never_transcoded_to_a_vector_format(#[case] accept: &str) {
    let mut martin = martin_with_a_vector_and_a_raster_source().await;

    let response = martin
        .get_with_headers("/png/0/0/0", &[("accept", accept)])
        .await;
    assert_eq!(response.status(), 406);
    assert_eq!(
        response.text(),
        "Source produces image/png, which does not match the Accept header"
    );

    martin.stop().await;
    martin.assert_log_contains(
        r#"ERROR error="Source produces image/png, which does not match the Accept header""#,
    );
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[rstest]
#[case::a_type_that_is_not_a_tile_format("text/html")]
#[case::a_tile_format_rejected_with_a_zero_quality("application/x-protobuf;q=0")]
#[case::a_wildcard_rejected_with_a_zero_quality("*/*;q=0")]
#[tokio::test]
async fn an_accept_header_naming_no_tile_format_is_rejected_before_the_source_is_resolved(
    #[case] accept: &str,
) {
    let mut martin = martin_with_a_vector_and_a_raster_source().await;

    let response = martin
        .get_with_headers("/nosuch/0/0/0", &[("accept", accept)])
        .await;
    assert_eq!(response.status(), 406);
    assert_eq!(
        response.text(),
        "Accept header does not contain any supported tile format"
    );

    martin.stop().await;
    martin.assert_log_contains(
        r#"ERROR error="Accept header does not contain any supported tile format""#,
    );
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[rstest]
#[case::just_above_the_maximum(2)]
#[case::far_above_the_maximum(10)]
#[tokio::test]
async fn a_zoom_the_source_does_not_cover_is_a_404_naming_the_range(#[case] zoom: u8) {
    let mut martin = martin_with_a_vector_and_a_raster_source().await;

    let response = martin.get(&format!("/png/{zoom}/0/0")).await;
    assert_eq!(response.status(), 404);
    assert_eq!(
        response.text(),
        format!("Zoom {zoom} is outside the supported range: png supports zoom 0-1")
    );

    martin.stop().await;
    martin.assert_log_contains(&format!(
        r#"ERROR error="Zoom {zoom} is outside the supported range: png supports zoom 0-1""#
    ));
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

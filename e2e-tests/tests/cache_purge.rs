//! The opt-in `DELETE /cache/{source_id}` route.

use indoc::formatdoc;
use martin_e2e_tests::{Martin, StaticFiles};

/// A minimal but valid MVT tile: one layer with one point feature.
fn mvt_tile() -> Vec<u8> {
    use mlt_core::TileLayer;
    use mlt_core::geo_types::{Geometry, Point};
    use mlt_core::mvt::tile_layers_to_mvt;

    let mut builder = TileLayer::builder("test", 4096).expect("layer builder");
    {
        let mut feature = builder.feature(Geometry::Point(Point::new(100, 200)));
        feature.id(Some(1));
        feature.finish().expect("finish feature");
    }
    tile_layers_to_mvt(vec![builder.finish()]).expect("encode MVT")
}

async fn upstream_with_one_tile() -> StaticFiles {
    let dir = tempfile::tempdir().expect("failed to create a temp dir");
    let tile = dir.path().join("tile.pbf");
    std::fs::write(&tile, mvt_tile()).expect("failed to write the tile");
    StaticFiles::serving(&[("0/0/0.pbf", tile)]).await
}

#[tokio::test]
async fn purging_a_source_makes_the_next_request_hit_the_upstream_again() {
    let upstream = upstream_with_one_tile().await;
    let config = formatdoc! {"
        endpoints:
          purge_cache: true
        passthrough:
          sources:
            proxy: {url}/{{z}}/{{x}}/{{y}}.pbf
    ", url = upstream.base_url()};
    let mut martin = Martin::builder()
        .config(&config)
        .start()
        .await
        .expect("failed to start martin");

    assert_eq!(martin.get("/proxy/0/0/0").await.status(), 200);
    assert_eq!(martin.get("/proxy/0/0/0").await.status(), 200);
    assert_eq!(
        upstream.request_log().await.lines().count(),
        1,
        "the second request must come from the tile cache"
    );

    let purged = martin.delete("/cache/proxy").await;
    assert_eq!(purged.status(), 200);
    assert_eq!(purged.text(), "Purged the cached tiles of source proxy");

    assert_eq!(martin.get("/proxy/0/0/0").await.status(), 200);
    assert_eq!(
        upstream.request_log().await.lines().count(),
        2,
        "after the purge the tile must be fetched from the upstream again"
    );

    assert_eq!(martin.delete("/cache/nope").await.status(), 404);

    martin.stop().await;
    martin.assert_log_contains("Invalidated tile cache for source: proxy");
    martin.assert_log_clean();
}

#[tokio::test]
async fn without_a_tile_cache_the_body_says_so() {
    let mut martin = Martin::builder()
        .config(
            formatdoc! {"
            endpoints:
              purge_cache: true
            cache: disable
            pmtiles:
              sources:
                pmt: tests/fixtures/pmtiles/png.pmtiles
        "}
            .as_str(),
        )
        .start()
        .await
        .expect("failed to start martin");

    let purged = martin.delete("/cache/pmt").await;
    assert_eq!(purged.status(), 200);
    assert_eq!(
        purged.text(),
        "Source pmt has no cached tiles, tile caching is disabled"
    );

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn the_route_is_absent_unless_enabled() {
    let mut martin = Martin::builder()
        .config(
            formatdoc! {"
            pmtiles:
              sources:
                pmt: tests/fixtures/pmtiles/png.pmtiles
        "}
            .as_str(),
        )
        .start()
        .await
        .expect("failed to start martin");

    assert_eq!(martin.delete("/cache/pmt").await.status(), 404);

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

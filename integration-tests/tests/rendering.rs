//! Raster tiles rendered from a `MapLibre` style, off a [`Cassette`] of what the style points at.
//!
//! Binary has to be built with `rendering`, which serves these routes on Linux only.

#![cfg(all(feature = "test-rendering", target_os = "linux"))]

use std::collections::HashSet;

use image::ImageFormat;
use martin_integration_tests::{Cassette, Martin, TestResponse, fixture};
use rstest::rstest;

const UPSTREAM: &str = "demotiles.maplibre.org";

async fn martin_rendering(cassette: &Cassette) -> Martin {
    let style = cassette.style(fixture("styles/maplibre_demo.json"));
    Martin::builder()
        .config(&format!(
            "styles:
  rendering:
    enabled: true
    workers: 2
  sources:
    maplibre_demo: {}
",
            style.display()
        ))
        .start()
        .await
        .expect("failed to start martin")
}

async fn stop_and_assert_log_clean(martin: &mut Martin) {
    martin.stop().await;
    martin.assert_log_contains("experimental feature rendering is enabled");
    martin.take_log_lines("[Render]");
    martin.assert_log_clean();
}

#[rstest]
#[case::the_world("/style/maplibre_demo/0/0/0.png")]
#[case::a_western_hemisphere("/style/maplibre_demo/1/0/0.png")]
#[case::an_eastern_hemisphere("/style/maplibre_demo/1/1/0.png")]
#[case::a_mid_zoom_tile("/style/maplibre_demo/5/15/15.png")]
#[tokio::test]
async fn a_style_renders_as_a_png_tile(#[case] path: &str) {
    let cassette = Cassette::serving(UPSTREAM).await;
    let mut martin = martin_rendering(&cassette).await;

    let response = martin.get(path).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.header("content-type"), Some("image/png"));
    assert_eq!(response.image_format(), ImageFormat::Png);
    assert_eq!(response.image_size(), (512, 512));

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

#[rstest]
#[case::the_world("/style/maplibre_demo/0/0/0.jpg")]
#[case::a_western_hemisphere("/style/maplibre_demo/1/0/0.jpg")]
#[tokio::test]
async fn a_style_renders_as_a_jpeg_tile(#[case] path: &str) {
    let cassette = Cassette::serving(UPSTREAM).await;
    let mut martin = martin_rendering(&cassette).await;

    let response = martin.get(path).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.header("content-type"), Some("image/jpeg"));
    assert_eq!(response.image_format(), ImageFormat::Jpeg);
    assert_eq!(response.image_size(), (512, 512));

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_rendered_tile_is_served_as_an_image() {
    let cassette = Cassette::serving(UPSTREAM).await;
    let mut martin = martin_rendering(&cassette).await;

    let response = martin.get("/style/maplibre_demo/0/0/0.png").await;
    insta::with_settings!({filters => vec![(r"(?m)^content-length: \d+$", "content-length: [LENGTH]")]}, {
        insta::assert_snapshot!(response.headers_snapshot());
    });

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_render_fetches_what_the_style_points_at() {
    let cassette = Cassette::serving(UPSTREAM).await;
    let mut martin = martin_rendering(&cassette).await;

    assert_eq!(
        martin.get("/style/maplibre_demo/0/0/0.png").await.status(),
        200
    );

    let mut fetched = cassette
        .request_log()
        .await
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    fetched.sort();
    fetched.dedup();
    insta::assert_snapshot!(fetched.join("\n"));

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

#[rstest]
#[case::jpeg("/style/maplibre_demo/0/0/0.jpeg", "/style/maplibre_demo/0/0/0.jpg")]
#[case::a_mid_zoom_tile(
    "/style/maplibre_demo/5/15/15.jpeg",
    "/style/maplibre_demo/5/15/15.jpg"
)]
#[tokio::test]
async fn the_jpeg_extension_redirects_to_jpg(#[case] path: &str, #[case] target: &str) {
    let cassette = Cassette::serving(UPSTREAM).await;
    let mut martin = martin_rendering(&cassette).await;

    let response = martin.get(path).await;
    assert_eq!(response.status(), 301);
    assert_eq!(response.header("location"), Some(target));

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn an_unknown_style_renders_nothing() {
    let cassette = Cassette::serving(UPSTREAM).await;
    let mut martin = martin_rendering(&cassette).await;

    let response = martin.get("/style/nope/0/0/0.png").await;
    assert_eq!(response.status(), 404);
    assert_eq!(response.text(), "No such style exists");

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

#[rstest]
#[case::past_the_zoom("/style/maplibre_demo/0/4000/4000.png")]
#[case::one_column_past_the_zoom("/style/maplibre_demo/1/2/0.png")]
#[tokio::test]
async fn coordinates_outside_their_zoom_render_nothing(#[case] path: &str) {
    let cassette = Cassette::serving(UPSTREAM).await;
    let mut martin = martin_rendering(&cassette).await;

    let response = martin.get(path).await;
    assert_eq!(response.status(), 400);
    assert_eq!(response.text(), "Invalid tile coordinates for zoom level");

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn neighbouring_tiles_render_differently() {
    let cassette = Cassette::serving(UPSTREAM).await;
    let mut martin = martin_rendering(&cassette).await;

    let west = martin.get("/style/maplibre_demo/1/0/0.png").await;
    let east = martin.get("/style/maplibre_demo/1/1/0.png").await;
    assert_ne!(west.body(), east.body());

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn tiles_requested_at_once_each_render_their_own_coordinates() {
    let cassette = Cassette::serving(UPSTREAM).await;
    let mut martin = martin_rendering(&cassette).await;

    let paths = [
        "/style/maplibre_demo/1/0/0.png",
        "/style/maplibre_demo/1/1/0.png",
        "/style/maplibre_demo/1/0/1.png",
        "/style/maplibre_demo/1/1/1.png",
    ];
    let (north_west, north_east, south_west, south_east) = tokio::join!(
        martin.get(paths[0]),
        martin.get(paths[1]),
        martin.get(paths[2]),
        martin.get(paths[3]),
    );
    let rendered = [north_west, north_east, south_west, south_east];

    for (path, response) in paths.iter().zip(&rendered) {
        assert_eq!(response.status(), 200, "{path} did not render");
        assert_eq!(response.image_size(), (512, 512), "{path} is not a tile");
    }
    let bodies = rendered
        .iter()
        .map(TestResponse::body)
        .collect::<HashSet<_>>();
    assert_eq!(bodies.len(), paths.len(), "the tiles are not all different");

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

//! Raster tiles and static images rendered from `MapLibre` styles, off a [`Cassette`] of what the
//! styles point at.
//!
//! Binary has to be built with `rendering`, which serves these routes on Linux only.

#![cfg(all(feature = "test-rendering", target_os = "linux"))]

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use image::ImageFormat;
use martin_integration_tests::{
    Cassette, Martin, TestResponse, assert_image_matches, assert_images_alike,
    assert_images_differ, fixture,
};
use rstest::rstest;
use test_each_file::test_each_path;

const UPSTREAMS: &[&str] = &[
    "demotiles.maplibre.org",
    "tiles.openfreemap.org",
    "openmaptiles.github.io",
];

const TILE_REFERENCES: &str = "rendering_references";
const CAMERA_REFERENCES: &str = "static_camera";

async fn martin_rendering(cassette: &Cassette) -> Martin {
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    Martin::builder()
        .config(&format!(
            "styles:
  rendering:
    enabled: true
    workers: 2
  sources:
    maplibre_demo: {}
    maptiler_basic: {}
",
            maplibre_demo.display(),
            maptiler_basic.display()
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

fn reference(group: &str, name: &str) -> PathBuf {
    fixture(group).join(name)
}

async fn rendered(martin: &Martin, path: &str) -> Vec<u8> {
    let response = martin.get(path).await;
    assert_eq!(response.status(), 200, "{path} did not render");
    response.body().to_vec()
}

#[rstest]
#[case::the_world("/style/maplibre_demo/0/0/0.png")]
#[case::a_western_hemisphere("/style/maplibre_demo/1/0/0.png")]
#[case::an_eastern_hemisphere("/style/maplibre_demo/1/1/0.png")]
#[case::a_mid_zoom_tile("/style/maplibre_demo/5/15/15.png")]
#[tokio::test]
async fn a_style_renders_as_a_png_tile(#[case] path: &str) {
    let cassette = Cassette::serving(UPSTREAMS).await;
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
    let cassette = Cassette::serving(UPSTREAMS).await;
    let mut martin = martin_rendering(&cassette).await;

    let response = martin.get(path).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.header("content-type"), Some("image/jpeg"));
    assert_eq!(response.image_format(), ImageFormat::Jpeg);
    assert_eq!(response.image_size(), (512, 512));

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

#[rstest]
#[case::the_world("maplibre_demo", "0/0/0", "png")]
#[case::a_western_hemisphere("maplibre_demo", "1/0/0", "png")]
#[case::another_style("maptiler_basic", "0/0/0", "png")]
#[case::the_world_as_jpeg("maplibre_demo", "0/0/0", "jpg")]
#[case::a_western_hemisphere_as_jpeg("maplibre_demo", "1/0/0", "jpg")]
#[case::another_style_as_jpeg("maptiler_basic", "0/0/0", "jpg")]
#[tokio::test]
async fn a_rendered_tile_matches_its_reference(
    #[case] style: &str,
    #[case] tile: &str,
    #[case] extension: &str,
) {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let mut martin = martin_rendering(&cassette).await;

    let body = rendered(&martin, &format!("/style/{style}/{tile}.{extension}")).await;
    let name = format!("{style}_{}.{extension}", tile.replace('/', "_"));
    assert_image_matches(reference(TILE_REFERENCES, &name), &body);

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_rendered_tile_is_served_as_an_image() {
    let cassette = Cassette::serving(UPSTREAMS).await;
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
    let cassette = Cassette::serving(UPSTREAMS).await;
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
    let cassette = Cassette::serving(UPSTREAMS).await;
    let mut martin = martin_rendering(&cassette).await;

    let response = martin.get(path).await;
    assert_eq!(response.status(), 301);
    assert_eq!(response.header("location"), Some(target));

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn an_unknown_style_renders_nothing() {
    let cassette = Cassette::serving(UPSTREAMS).await;
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
    let cassette = Cassette::serving(UPSTREAMS).await;
    let mut martin = martin_rendering(&cassette).await;

    let response = martin.get(path).await;
    assert_eq!(response.status(), 400);
    assert_eq!(response.text(), "Invalid tile coordinates for zoom level");

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn neighbouring_tiles_render_differently() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let mut martin = martin_rendering(&cassette).await;

    let west = martin.get("/style/maplibre_demo/1/0/0.png").await;
    let east = martin.get("/style/maplibre_demo/1/1/0.png").await;
    assert_ne!(west.body(), east.body());

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn tiles_requested_at_once_each_render_their_own_coordinates() {
    let cassette = Cassette::serving(UPSTREAMS).await;
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

#[tokio::test]
async fn a_static_image_renders_as_a_jpeg() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let mut martin = martin_rendering(&cassette).await;

    let response = martin
        .get("/style/maplibre_demo/static/0,0,0/200x200.jpg")
        .await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.header("content-type"), Some("image/jpeg"));
    assert_eq!(response.image_format(), ImageFormat::Jpeg);

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

#[rstest]
#[case::a_centered_camera("0,0,0/200x200", "center_z0")]
#[case::a_zoomed_in_camera("0,0,3/200x200", "center_z3")]
#[case::a_camera_off_the_origin("13.4,52.5,4/200x200", "center_berlin_z4")]
#[case::a_rotated_camera("0,0,2@90/200x200", "bearing_90")]
#[case::a_tilted_camera("0,0,2@0,45/200x200", "pitch_45")]
#[case::a_bounding_box("-30,-30,30,30/200x200", "bbox_pm30")]
#[case::a_bounding_box_off_the_origin("-10,40,30,60/200x200", "bbox_europe")]
#[tokio::test]
async fn a_static_image_matches_its_reference(#[case] camera: &str, #[case] name: &str) {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let mut martin = martin_rendering(&cassette).await;

    let body = rendered(
        &martin,
        &format!("/style/maplibre_demo/static/{camera}.png"),
    )
    .await;
    assert_image_matches(reference(CAMERA_REFERENCES, &format!("{name}.png")), &body);

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

#[rstest]
#[case::zoom("0,0,0/200x200", "0,0,3/200x200")]
#[case::bearing("0,0,2@0/200x200", "0,0,2@90/200x200")]
#[case::pitch("0,0,2@0,0/200x200", "0,0,2@0,45/200x200")]
#[case::the_center("0,0,4/200x200", "13.4,52.5,4/200x200")]
#[case::the_bounding_box("-20,-10,20,10/200x200", "-10,40,30,60/200x200")]
#[tokio::test]
async fn a_camera_option_changes_the_static_image(#[case] one: &str, #[case] other: &str) {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let mut martin = martin_rendering(&cassette).await;

    let one = rendered(&martin, &format!("/style/maplibre_demo/static/{one}.png")).await;
    let other = rendered(&martin, &format!("/style/maplibre_demo/static/{other}.png")).await;
    assert_images_differ(&one, &other);

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_bounding_box_frames_what_a_center_and_zoom_frame() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let mut martin = martin_rendering(&cassette).await;

    let bbox = rendered(
        &martin,
        "/style/maplibre_demo/static/-30,-30,30,30/200x200.png",
    )
    .await;
    let center = rendered(&martin, "/style/maplibre_demo/static/0,0,2.16/200x200.png").await;
    assert_images_alike(&bbox, &center);

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_doubled_pixel_ratio_doubles_the_static_image() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let mut martin = martin_rendering(&cassette).await;

    let one_x = martin
        .get("/style/maplibre_demo/static/0,0,0/100x100.png")
        .await;
    let two_x = martin
        .get("/style/maplibre_demo/static/0,0,0/100x100@2x.png")
        .await;
    assert_eq!(one_x.image_size(), (100, 100));
    assert_eq!(two_x.image_size(), (200, 200));

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

#[rstest]
#[case::an_empty_body(b"")]
#[case::an_overlay_without_features(br#"{"type": "FeatureCollection", "features": []}"#)]
#[tokio::test]
async fn a_post_without_overlays_renders_the_base_map(#[case] body: &[u8]) {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let mut martin = martin_rendering(&cassette).await;

    let response = martin
        .post_json("/style/maplibre_demo/static/0,0,0/200x200.png", body)
        .await;
    assert_eq!(response.status(), 200);
    assert_image_matches(
        reference(CAMERA_REFERENCES, "center_z0.png"),
        response.body(),
    );

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

async fn assert_overlay_matches_its_reference(scenario: &Path, camera: &str, group: &str) {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let mut martin = martin_rendering(&cassette).await;

    let overlay = std::fs::read(scenario)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", scenario.display()));
    let response = martin
        .post_json(
            &format!("/style/maplibre_demo/static/{camera}.png"),
            &overlay,
        )
        .await;
    assert_eq!(
        response.status(),
        200,
        "{} did not render",
        scenario.display()
    );
    let name = scenario
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .expect("a scenario file has a name");
    assert_image_matches(
        fixture("static_overlays")
            .join(group)
            .join(format!("{name}.png")),
        response.body(),
    );

    stop_and_assert_log_clean(&mut martin).await;
    cassette.assert_no_misses();
}

test_each_path! {
    #[tokio::test]
    async in "tests/fixtures/static_overlays/input"
    as overlays
    => async |scenario: &Path| assert_overlay_matches_its_reference(scenario, "0,0,2/200x200", "1x").await
}

test_each_path! {
    #[tokio::test]
    async in "tests/fixtures/static_overlays/input"
    as overlays_at_a_doubled_pixel_ratio
    => async |scenario: &Path| assert_overlay_matches_its_reference(scenario, "0,0,2/200x200@2x", "2x").await
}

test_each_path! {
    #[tokio::test]
    async in "tests/fixtures/static_overlays/input"
    as overlays_through_a_tilted_camera
    => async |scenario: &Path| assert_overlay_matches_its_reference(scenario, "0,0,2@0,60/200x200", "1x_pitch").await
}

test_each_path! {
    #[tokio::test]
    async in "tests/fixtures/static_overlays/input"
    as overlays_through_a_rotated_camera
    => async |scenario: &Path| assert_overlay_matches_its_reference(scenario, "0,0,2@45/200x200", "1x_bearing").await
}

//! Raster tiles and static images rendered from `MapLibre` styles, off a [`Cassette`] of what the
//! styles point at.
//!
//! Binary has to be built with `rendering`, which serves these routes on Linux only.

#![cfg(all(feature = "test-rendering", target_os = "linux"))]

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use image::{ColorType, ImageFormat};
use indoc::formatdoc;
use martin_e2e_tests::{
    Cassette, Martin, StartError, TestResponse, assert_image_matches, assert_images_alike, assert_images_differ,
    fixture,
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

/// Colour type and bit depth the PNG `body` stores its pixels in, before any decoder expands them.
fn png_pixel_format(body: &[u8]) -> String {
    let reader = png::Decoder::new(std::io::Cursor::new(body))
        .read_info()
        .expect("response body is not a PNG");
    let info = reader.info();
    format!("{:?} {:?}", info.color_type, info.bit_depth)
}

/// `Lossy` or `Lossless`, how the WebP `body` is encoded.
fn webp_encoding(body: &[u8]) -> &'static str {
    let mut webp = image_webp::WebPDecoder::new(std::io::Cursor::new(body)).expect("response body is not a WebP");
    if webp.is_lossy() { "Lossy" } else { "Lossless" }
}

/// Stop martin and consume the log lines rendering always emits, leaving the log clean for the
/// assertion dropping a [`Martin`] makes.
async fn stop_and_take_rendering_log(martin: &mut Martin) {
    martin.stop().await;
    martin.take_log_lines("[Render]");
}

fn reference(group: &str, name: &str) -> PathBuf {
    fixture(group).join(name)
}

async fn rendered(martin: &Martin, path: &str) -> Vec<u8> {
    let response = martin.get(path).await;
    assert_eq!(response.status(), 200, "{path} did not render: {}", response.text());
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
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let response = martin.get(path).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.header("content-type"), Some("image/png"));
    assert_eq!(response.image_format(), ImageFormat::Png);
    assert_eq!(response.image_size(), (512, 512));

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[rstest]
#[case::the_world("/style/maplibre_demo/0/0/0.jpg")]
#[case::a_western_hemisphere("/style/maplibre_demo/1/0/0.jpg")]
#[tokio::test]
async fn a_style_renders_as_a_jpeg_tile(#[case] path: &str) {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let response = martin.get(path).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.header("content-type"), Some("image/jpeg"));
    assert_eq!(response.image_format(), ImageFormat::Jpeg);
    assert_eq!(response.image_size(), (512, 512));

    stop_and_take_rendering_log(&mut martin).await;
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
async fn a_rendered_tile_matches_its_reference(#[case] style: &str, #[case] tile: &str, #[case] extension: &str) {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let body = rendered(&martin, &format!("/style/{style}/{tile}.{extension}")).await;
    let name = format!("{style}_{}.{extension}", tile.replace('/', "_"));
    assert_image_matches(reference(TILE_REFERENCES, &name), &body);

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_rendered_tile_is_served_as_an_image() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let response = martin.get("/style/maplibre_demo/0/0/0.png").await;
    insta::with_settings!({filters => vec![(r"(?m)^content-length: \d+$", "content-length: [LENGTH]")]}, {
        insta::assert_snapshot!(response.headers_snapshot());
    });

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_render_fetches_what_the_style_points_at() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    assert_eq!(martin.get("/style/maplibre_demo/0/0/0.png").await.status(), 200);

    let mut fetched = cassette
        .request_log()
        .await
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    fetched.sort();
    fetched.dedup();
    insta::assert_snapshot!(fetched.join("\n"));

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[rstest]
#[case::jpeg("/style/maplibre_demo/0/0/0.jpeg", "/style/maplibre_demo/0/0/0.jpg")]
#[case::a_mid_zoom_tile("/style/maplibre_demo/5/15/15.jpeg", "/style/maplibre_demo/5/15/15.jpg")]
#[case::a_pixel_ratio("/style/maplibre_demo/0/0/0@2x.jpeg", "/style/maplibre_demo/0/0/0@2x.jpg")]
#[tokio::test]
async fn the_jpeg_extension_redirects_to_jpg(#[case] path: &str, #[case] target: &str) {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let response = martin.get(path).await;
    assert_eq!(response.status(), 301);
    assert_eq!(response.header("location"), Some(target));

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn an_unknown_style_renders_nothing() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let response = martin.get("/style/nope/0/0/0.png").await;
    assert_eq!(response.status(), 404);
    assert_eq!(response.text(), "No such style exists");

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[rstest]
#[case::past_the_zoom("/style/maplibre_demo/0/4000/4000.png")]
#[case::one_column_past_the_zoom("/style/maplibre_demo/1/2/0.png")]
#[tokio::test]
async fn coordinates_outside_their_zoom_render_nothing(#[case] path: &str) {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let response = martin.get(path).await;
    assert_eq!(response.status(), 400);
    assert_eq!(response.text(), "Invalid tile coordinates for zoom level");

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[rstest]
#[case::a_zero_pixel_ratio("/style/maplibre_demo/0/0/0@0x.png")]
#[case::a_pixel_ratio_without_a_number("/style/maplibre_demo/0/0/0@x.png")]
#[case::a_fractional_pixel_ratio("/style/maplibre_demo/0/0/0@1.5x.png")]
#[case::a_pixel_ratio_without_the_x("/style/maplibre_demo/0/0/0@2.png")]
#[case::a_pixel_ratio_with_a_leading_zero("/style/maplibre_demo/0/0/0@02x.png")]
#[case::a_pixel_ratio_past_u8("/style/maplibre_demo/0/0/0@256x.png")]
#[case::a_row_that_is_not_a_number("/style/maplibre_demo/0/0/a@2x.png")]
#[case::a_redirect_with_a_zero_pixel_ratio("/style/maplibre_demo/0/0/0@0x.jpeg")]
#[tokio::test]
async fn a_malformed_pixel_ratio_is_not_found(#[case] path: &str) {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let response = martin.get(path).await;
    assert_eq!(response.status(), 404);

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[rstest]
#[case::not_in_the_default("/style/maplibre_demo/0/0/0@3x.png", "Pixel ratio @3x is not served, only @1x, @2x")]
#[case::above_the_max("/style/maplibre_demo/0/0/0@5x.png", "Pixel ratio @5x is not served, only @1x, @2x")]
#[case::outside_the_zoom("/style/maplibre_demo/0/4000/4000@2x.png", "Invalid tile coordinates for zoom level")]
#[tokio::test]
async fn a_pixel_ratio_tile_that_is_not_served_is_a_bad_request(#[case] path: &str, #[case] reason: &str) {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let response = martin.get(path).await;
    assert_eq!(response.status(), 400);
    assert_eq!(response.text(), reason);

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[rstest]
#[case::one_x("/style/maplibre_demo/0/0/0@1x.png", 512)]
#[case::two_x("/style/maplibre_demo/0/0/0@2x.png", 1024)]
#[case::three_x("/style/maplibre_demo/5/15/15@3x.png", 1536)]
#[case::four_x("/style/maplibre_demo/1/0/0@4x.png", 2048)]
#[case::two_x_as_jpeg("/style/maplibre_demo/0/0/0@2x.jpg", 1024)]
#[tokio::test]
async fn a_pixel_ratio_multiplies_the_tile_size(#[case] path: &str, #[case] size: u32) {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
                pixel_ratios: [1, 2, 3, 4]
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let response = martin.get(path).await;
    assert_eq!(response.status(), 200, "{path} did not render");
    assert_eq!(response.image_size(), (size, size));

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn tiles_of_mixed_pixel_ratios_requested_at_once_each_render_at_their_size() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
                pixel_ratios: [1, 2, 3]
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let paths = [
        ("/style/maplibre_demo/1/0/0.png", 512),
        ("/style/maplibre_demo/1/0/0@2x.png", 1024),
        ("/style/maplibre_demo/1/1/0@2x.png", 1024),
        ("/style/maplibre_demo/1/1/0.png", 512),
        ("/style/maplibre_demo/1/0/1@3x.png", 1536),
    ];
    let rendered = tokio::join!(
        martin.get(paths[0].0),
        martin.get(paths[1].0),
        martin.get(paths[2].0),
        martin.get(paths[3].0),
        martin.get(paths[4].0),
    );
    let rendered = [rendered.0, rendered.1, rendered.2, rendered.3, rendered.4];

    for ((path, size), response) in paths.iter().zip(&rendered) {
        assert_eq!(response.status(), 200, "{path} did not render");
        assert_eq!(response.image_size(), (*size, *size), "{path}");
    }

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn configured_pixel_ratios_limit_the_tiles() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
                pixel_ratios: [1, 3]
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let three_x = martin.get("/style/maplibre_demo/5/15/15@3x.png").await;
    assert_eq!(three_x.status(), 200);
    assert_eq!(three_x.image_size(), (1536, 1536));
    let two_x = martin.get("/style/maplibre_demo/0/0/0@2x.png").await;
    assert_eq!(two_x.status(), 400);
    assert_eq!(two_x.text(), "Pixel ratio @2x is not served, only @1x, @3x");

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_pixel_ratio_above_4_fails_startup() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let error = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
                pixel_ratios: [1, 5]
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect_err("martin must reject pixel_ratios: [1, 5]");
    let StartError::EarlyExit { status, log } = error else {
        panic!("expected an early exit, got: {error}");
    };
    assert!(!status.success(), "exit status must be a failure: {status}");
    assert!(
        log.contains("pixel ratio must be 1 to 4, got 5"),
        "log must name the invalid value; log:\n{log}"
    );
}

#[rstest]
#[case::the_world("/style/maplibre_demo/0/0/0.png", 256)]
#[case::the_world_at_2x("/style/maplibre_demo/0/0/0@2x.png", 512)]
#[case::the_world_at_3x("/style/maplibre_demo/0/0/0@3x.png", 768)]
#[case::a_tile("/style/maplibre_demo/5/15/15.png", 256)]
#[case::a_tile_at_2x("/style/maplibre_demo/5/15/15@2x.png", 512)]
#[case::a_tile_as_jpeg("/style/maplibre_demo/1/0/0.jpg", 256)]
#[tokio::test]
async fn a_configured_tile_size_sizes_the_tiles(#[case] path: &str, #[case] size: u32) {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
                tile_size: 256
                pixel_ratios: [1, 2, 3]
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let response = martin.get(path).await;
    assert_eq!(response.status(), 200, "{path} did not render");
    assert_eq!(response.image_size(), (size, size));

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn an_unsupported_tile_size_fails_startup() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let error = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
                tile_size: 300
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect_err("martin must reject tile_size: 300");
    let StartError::EarlyExit { status, log } = error else {
        panic!("expected an early exit, got: {error}");
    };
    assert!(!status.success(), "exit status must be a failure: {status}");
    assert!(
        log.contains("tile size must be 256 or 512, got 300"),
        "log must name the allowed tile sizes; log:\n{log}"
    );
}

#[tokio::test]
async fn one_renderer_per_worker_still_renders_each_style() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
                renderers_per_worker: 1
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    for style in ["maplibre_demo", "maptiler_basic", "maplibre_demo"] {
        let body = rendered(&martin, &format!("/style/{style}/0/0/0.png")).await;
        assert_image_matches(reference(TILE_REFERENCES, &format!("{style}_0_0_0.png")), &body);
    }

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

/// With one renderer per worker, rendering another style in between drops the renderer of the
/// first, so rendering the first again loads it into a new renderer that asks for its tile again.
/// `MapLibre` Native's ambient cache outlives renderers, and answers that ask while the tile is
/// fresh, unless it is disabled.
#[rstest]
#[case::by_default("", 1)]
#[case::disabled("ambient_cache_size_mb: 0", 2)]
#[tokio::test]
async fn the_ambient_cache_answers_a_reloaded_renderer(#[case] ambient_cache: &str, #[case] fetches: usize) {
    // Without a freshness header, MapLibre Native asks the server again even for what it has
    // cached.
    let cassette = Cassette::serving_fresh_for(UPSTREAMS, 3600).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 1
                renderers_per_worker: 1
                {ambient_cache}
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    for style in ["maplibre_demo", "maptiler_basic", "maplibre_demo"] {
        let body = rendered(&martin, &format!("/style/{style}/0/0/0.png")).await;
        assert_image_matches(reference(TILE_REFERENCES, &format!("{style}_0_0_0.png")), &body);
    }

    let log = cassette.request_log().await;
    let tile = "GET /demotiles.maplibre.org/tiles/0/0/0.pbf";
    assert_eq!(
        log.lines().filter(|line| *line == tile).count(),
        fetches,
        "times maplibre_demo's tile was fetched; requests:\n{log}"
    );

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn neighbouring_tiles_render_differently() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let west = martin.get("/style/maplibre_demo/1/0/0.png").await;
    let east = martin.get("/style/maplibre_demo/1/1/0.png").await;
    assert_ne!(west.body(), east.body());

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn tiles_requested_at_once_each_render_their_own_coordinates() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

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
    let bodies = rendered.iter().map(TestResponse::body).collect::<HashSet<_>>();
    assert_eq!(bodies.len(), paths.len(), "the tiles are not all different");

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_static_image_renders_as_a_jpeg() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let response = martin.get("/style/maplibre_demo/static/0,0,0/200x200.jpg").await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.header("content-type"), Some("image/jpeg"));
    assert_eq!(response.image_format(), ImageFormat::Jpeg);

    stop_and_take_rendering_log(&mut martin).await;
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
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let body = rendered(&martin, &format!("/style/maplibre_demo/static/{camera}.png")).await;
    assert_image_matches(reference(CAMERA_REFERENCES, &format!("{name}.png")), &body);

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[rstest]
#[case::zoom("0,0,0/200x200", "0,0,3/200x200")]
#[case::a_zoom_below_0("0,0,0/200x200", "0,0,-1/200x200")]
#[case::bearing("0,0,2@0/200x200", "0,0,2@90/200x200")]
#[case::pitch("0,0,2@0,0/200x200", "0,0,2@0,45/200x200")]
#[case::the_center("0,0,4/200x200", "13.4,52.5,4/200x200")]
#[case::the_bounding_box("-20,-10,20,10/200x200", "-10,40,30,60/200x200")]
#[tokio::test]
async fn a_camera_option_changes_the_static_image(#[case] one: &str, #[case] other: &str) {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let one = rendered(&martin, &format!("/style/maplibre_demo/static/{one}.png")).await;
    let other = rendered(&martin, &format!("/style/maplibre_demo/static/{other}.png")).await;
    assert_images_differ(&one, &other);

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_static_image_zoomed_too_far_below_0_is_a_bad_request() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let response = martin.get("/style/maplibre_demo/static/0,0,-4/200x200.png").await;
    assert_eq!(response.status(), 400);
    insta::assert_snapshot!(response.text(), @"Zoom -4 would draw more than 2048 pixels a side, zoom in or render a smaller image");

    stop_and_take_rendering_log(&mut martin).await;
}

#[tokio::test]
async fn a_bounding_box_frames_what_a_center_and_zoom_frame() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let bbox = rendered(&martin, "/style/maplibre_demo/static/-30,-30,30,30/200x200.png").await;
    let center = rendered(&martin, "/style/maplibre_demo/static/0,0,2.16/200x200.png").await;
    assert_images_alike(&bbox, &center);

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_doubled_pixel_ratio_doubles_the_static_image() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let one_x = martin.get("/style/maplibre_demo/static/0,0,0/100x100.png").await;
    let two_x = martin.get("/style/maplibre_demo/static/0,0,0/100x100@2x.png").await;
    assert_eq!(one_x.image_size(), (100, 100));
    assert_eq!(two_x.image_size(), (200, 200));

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

const ENCODED_PATHS: &[&str] = &[
    "/style/maplibre_demo/0/0/0.png",
    "/style/maplibre_demo/5/15/15.png",
    "/style/maptiler_basic/0/0/0.png",
    "/style/maplibre_demo/static/0,0,0/200x200.png",
    "/style/maplibre_demo/0/0/0.jpg",
];

const QUALITY_PATHS: &[&str] = &[
    "/style/maplibre_demo/0/0/0.webp",
    "/style/maptiler_basic/0/0/0.webp",
    "/style/maplibre_demo/static/0,0,0/200x200.webp",
    "/style/maplibre_demo/0/0/0.jpg",
];

/// `path: status, content type, encoding` for each of `paths`. The encoding is the colour type and
/// bit depth of a PNG, and whether a WebP is lossy.
async fn encodings(martin: &Martin, paths: &[&str]) -> String {
    let mut lines = Vec::new();
    for path in paths {
        let response = martin.get(path).await;
        let format = response.image_format();
        let pixels = if format == ImageFormat::Png {
            png_pixel_format(response.body())
        } else if format == ImageFormat::WebP {
            webp_encoding(response.body()).to_owned()
        } else {
            "-".to_owned()
        };
        lines.push(format!(
            "{path}: {} {} {pixels}",
            response.status(),
            response.header("content-type").unwrap_or_default()
        ));
    }
    lines.join("\n")
}

#[tokio::test]
async fn rendered_pngs_are_indexed_by_default() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    insta::assert_snapshot!(encodings(&martin, ENCODED_PATHS).await, @"
    /style/maplibre_demo/0/0/0.png: 200 image/png Indexed Eight
    /style/maplibre_demo/5/15/15.png: 200 image/png Indexed Eight
    /style/maptiler_basic/0/0/0.png: 200 image/png Indexed Eight
    /style/maplibre_demo/static/0,0,0/200x200.png: 200 image/png Indexed Eight
    /style/maplibre_demo/0/0/0.jpg: 200 image/jpeg -
    ");

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn the_rendering_shorthand_indexes_pngs_too() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering: true
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    insta::assert_snapshot!(encodings(&martin, ENCODED_PATHS).await, @"
    /style/maplibre_demo/0/0/0.png: 200 image/png Indexed Eight
    /style/maplibre_demo/5/15/15.png: 200 image/png Indexed Eight
    /style/maptiler_basic/0/0/0.png: 200 image/png Indexed Eight
    /style/maplibre_demo/static/0,0,0/200x200.png: 200 image/png Indexed Eight
    /style/maplibre_demo/0/0/0.jpg: 200 image/jpeg -
    ");

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_disabled_png_palette_keeps_rgba() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
                png_palette: false
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    insta::assert_snapshot!(encodings(&martin, ENCODED_PATHS).await, @"
    /style/maplibre_demo/0/0/0.png: 200 image/png Rgba Eight
    /style/maplibre_demo/5/15/15.png: 200 image/png Rgba Eight
    /style/maptiler_basic/0/0/0.png: 200 image/png Rgba Eight
    /style/maplibre_demo/static/0,0,0/200x200.png: 200 image/png Rgba Eight
    /style/maplibre_demo/0/0/0.jpg: 200 image/jpeg -
    ");

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_small_png_palette_packs_fewer_bits() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
                png_palette:
                  max_colors: 4
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    insta::assert_snapshot!(encodings(&martin, ENCODED_PATHS).await, @"
    /style/maplibre_demo/0/0/0.png: 200 image/png Indexed Two
    /style/maplibre_demo/5/15/15.png: 200 image/png Indexed Two
    /style/maptiler_basic/0/0/0.png: 200 image/png Indexed Two
    /style/maplibre_demo/static/0,0,0/200x200.png: 200 image/png Indexed Two
    /style/maplibre_demo/0/0/0.jpg: 200 image/jpeg -
    ");

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[rstest]
#[case::too_few(1)]
#[case::too_many(257)]
#[tokio::test]
async fn a_png_palette_size_outside_png_fails_startup(#[case] max_colors: u32) {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let error = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
                png_palette:
                  max_colors: {max_colors}
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect_err("martin must reject the palette size");
    let StartError::EarlyExit { status, log } = error else {
        panic!("expected an early exit, got: {error}");
    };
    assert!(!status.success(), "exit status must be a failure: {status}");
    assert!(
        log.contains("max_colors must be 2 to 256"),
        "log must name the invalid option; log:\n{log}"
    );
}

#[tokio::test]
async fn rendered_webps_are_lossy_by_default() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    insta::assert_snapshot!(encodings(&martin, QUALITY_PATHS).await, @"
    /style/maplibre_demo/0/0/0.webp: 200 image/webp Lossy
    /style/maptiler_basic/0/0/0.webp: 200 image/webp Lossy
    /style/maplibre_demo/static/0,0,0/200x200.webp: 200 image/webp Lossy
    /style/maplibre_demo/0/0/0.jpg: 200 image/jpeg -
    ");

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_lossless_webp_quality_keeps_webps_lossless() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
                quality:
                  webp: lossless
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    insta::assert_snapshot!(encodings(&martin, QUALITY_PATHS).await, @"
    /style/maplibre_demo/0/0/0.webp: 200 image/webp Lossless
    /style/maptiler_basic/0/0/0.webp: 200 image/webp Lossless
    /style/maplibre_demo/static/0,0,0/200x200.webp: 200 image/webp Lossless
    /style/maplibre_demo/0/0/0.jpg: 200 image/jpeg -
    ");

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_lower_quality_serves_smaller_images() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut served = Vec::new();
    for quality in [10, 100] {
        let mut martin = Martin::builder()
            .config(&formatdoc! {"
                styles:
                  rendering:
                    enabled: true
                    workers: 2
                    quality:
                      webp: {quality}
                      jpeg: {quality}
                  sources:
                    maplibre_demo: {maplibre_demo}
                    maptiler_basic: {maptiler_basic}
            "})
            .start()
            .await
            .expect("failed to start martin");
        let mut responses = Vec::new();
        for path in QUALITY_PATHS {
            responses.push(martin.get(path).await);
        }
        stop_and_take_rendering_log(&mut martin).await;
        served.push(responses);
    }

    let [low, high] = served.as_slice() else {
        unreachable!("two qualities were served");
    };
    for ((path, low), high) in QUALITY_PATHS.iter().zip(low).zip(high) {
        for response in [low, high] {
            assert_eq!(response.status(), 200, "{path}");
            // Decodes every pixel, not only the header.
            let color = response.image_color();
            assert!(matches!(color, ColorType::Rgb8 | ColorType::Rgba8), "{path}: {color:?}");
            if response.image_format() == ImageFormat::WebP {
                assert_eq!(webp_encoding(response.body()), "Lossy", "{path}");
            }
        }
        assert_eq!(low.image_size(), high.image_size(), "{path}");
        assert!(
            low.body().len() < high.body().len(),
            "{path}: quality 10 is {} bytes, quality 100 is {} bytes",
            low.body().len(),
            high.body().len()
        );
    }
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_webp_quality_outside_1_to_100_fails_startup() {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maplibre_demo = maplibre_demo.display();
    let error = Martin::builder()
        .env("RUST_LOG", "martin=error")
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
                quality:
                  webp: 0
              sources:
                maplibre_demo: {maplibre_demo}
        "})
        .start()
        .await
        .expect_err("martin must reject the quality");
    let StartError::EarlyExit { status, log } = error else {
        panic!("expected an early exit, got: {error}");
    };
    assert!(!status.success(), "exit status must be a failure: {status}");
    insta::with_settings!({filters => vec![(r"/\S*/config\.yaml", "[CONFIG]")]}, {
        insta::assert_snapshot!(log, @r#"
        ERROR martin::config::yaml (https://maplibre.org/martin/config-file/)

          × quality must be 1 to 100 or "lossless", not 0
           ╭─[[CONFIG]:6:13]
         5 │     quality:
         6 │       webp: 0
           ·             ─
         7 │   sources:
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        "#);
    });
}

#[rstest]
#[case::an_empty_body(b"")]
#[case::an_overlay_without_features(br#"{"type": "FeatureCollection", "features": []}"#)]
#[tokio::test]
async fn a_post_without_overlays_renders_the_base_map(#[case] body: &[u8]) {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let response = martin
        .post_json("/style/maplibre_demo/static/0,0,0/200x200.png", body)
        .await;
    assert_eq!(response.status(), 200);
    assert_image_matches(reference(CAMERA_REFERENCES, "center_z0.png"), response.body());

    stop_and_take_rendering_log(&mut martin).await;
    cassette.assert_no_misses();
}

async fn assert_overlay_matches_its_reference(scenario: &Path, camera: &str, group: &str) {
    let cassette = Cassette::serving(UPSTREAMS).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maptiler_basic = cassette.style(fixture("styles/src2/maptiler_basic.json"));
    let maplibre_demo = maplibre_demo.display();
    let maptiler_basic = maptiler_basic.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 2
                png_palette: false
              sources:
                maplibre_demo: {maplibre_demo}
                maptiler_basic: {maptiler_basic}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let overlay = std::fs::read(scenario).expect("failed to read the overlay scenario");
    let response = martin
        .post_json(&format!("/style/maplibre_demo/static/{camera}.png"), &overlay)
        .await;
    assert_eq!(response.status(), 200, "{} did not render", scenario.display());
    let name = scenario
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .expect("a scenario file has a name");
    assert_image_matches(
        fixture("static_overlays").join(group).join(format!("{name}.png")),
        response.body(),
    );

    stop_and_take_rendering_log(&mut martin).await;
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

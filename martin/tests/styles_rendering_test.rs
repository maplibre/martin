#![cfg(all(feature = "rendering", target_os = "linux", feature = "styles"))]

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use actix_web::http::header::{CONTENT_TYPE, LOCATION};
use actix_web::test::{TestRequest, call_service, read_body};
use martin::config::file::srv::SrvConfig;
use rstest::rstest;
use test_each_file::test_each_path;

pub mod utils;
pub use utils::*;

/// Upstreams reverse-proxied by the rendering cassette and their plain-HTTP
/// ports. Must match `_run-render-proxy` in the justfile and `PROXIED_HOSTS`
/// in `martin-core/tests/rendering_test.rs`.
const PROXIED_HOSTS: &[(&str, u16)] = &[
    ("https://demotiles.maplibre.org", 18081),
    ("https://tiles.openfreemap.org", 18082),
];

/// Styles config used by every rendering test. It points at a copy of
/// `maplibre_demo.json` whose upstream URLs are rewritten to the mitmproxy
/// cassette (`just with-render-cache` / `just seed-render-fixtures`) whenever
/// that proxy is listening, so renders replay recorded tiles instead of hitting
/// the network. Without the proxy the original live URLs are kept, so the test
/// still works when run bare. The rewritten style lives in a leaked temp dir
/// for the lifetime of the test process.
static CONFIG_STYLES: LazyLock<String> = LazyLock::new(build_styles_config);

fn build_styles_config() -> String {
    let original = Path::new("../tests/fixtures/styles/maplibre_demo.json");
    let mut body = std::fs::read_to_string(original).expect("read maplibre_demo.json");
    for &(host, port) in PROXIED_HOSTS {
        if proxy_listening(port) {
            body = body.replace(host, &format!("http://127.0.0.1:{port}"));
        }
    }
    let dir = tempfile::tempdir().expect("create temp style dir").keep();
    let style_path = dir.join("maplibre_demo.json");
    std::fs::write(&style_path, body).expect("write rewritten style");
    format!(
        "styles:\n  rendering: true\n  sources:\n    maplibre_demo: {}\n",
        style_path.display()
    )
}

/// True when the cassette reverse-proxy is accepting connections on `port`.
fn proxy_listening(port: u16) -> bool {
    use std::net::{Ipv4Addr, SocketAddr, TcpStream};
    use std::time::Duration;

    TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_millis(200),
    )
    .is_ok()
}

macro_rules! create_app {
    ($sources:expr) => {{
        let state = mock_sources(mock_cfg($sources).await).await.0;
        let app = ::actix_web::App::new()
            .app_data(actix_web::web::Data::new(
                ::martin::srv::Catalog::new(
                    #[cfg(any(feature = "sprites", feature = "fonts", feature = "styles"))]
                    &state,
                )
                .unwrap(),
            ))
            .app_data(actix_web::web::Data::new(SrvConfig::default()));

        #[cfg(feature = "_tiles")]
        let app = app.app_data(actix_web::web::Data::new(state.tile_manager.clone()));

        #[cfg(feature = "sprites")]
        let app = app.app_data(actix_web::web::Data::new(state.sprites));

        let app = app
            .app_data(actix_web::web::Data::new(state.styles))
            .configure(|c| ::martin::srv::router(c, &SrvConfig::default()));

        ::actix_web::test::init_service(app).await
    }};
}

fn test_get(path: &str) -> TestRequest {
    TestRequest::get().uri(path)
}

/// Full PNG magic: 8 bytes
const PNG_MAGIC: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
/// JPEG magic: first 3 bytes
const JPEG_MAGIC: &[u8] = &[0xFF, 0xD8, 0xFF];

#[rstest]
#[case::single_style(CONFIG_STYLES.as_str(), "/style/maplibre_demo/0/0/0.png")]
#[case::single_style_zoom_1(CONFIG_STYLES.as_str(), "/style/maplibre_demo/1/0/0.png")]
#[case::single_style_corner(CONFIG_STYLES.as_str(), "/style/maplibre_demo/1/1/0.png")]
#[case::single_style_mid_zoom(CONFIG_STYLES.as_str(), "/style/maplibre_demo/5/15/15.png")]
#[tokio::test]
#[tracing_test::traced_test]
async fn render_tile_png(#[case] config: &str, #[case] path: &str) {
    let app = create_app! { config };

    let req = test_get(path).to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;

    assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), "image/png");

    let body = read_body(response).await;
    assert!(
        body.len() > 1000,
        "PNG should have reasonable size for {path}, got {}",
        body.len()
    );

    // Verify full PNG magic (8 bytes)
    assert_eq!(&body[..8], PNG_MAGIC, "Response is not a valid PNG");

    // Decode and verify dimensions
    let img = image::load_from_memory_with_format(&body, image::ImageFormat::Png)
        .expect("Failed to decode PNG response");
    assert_eq!(
        (img.width(), img.height()),
        (512, 512),
        "Tile must be 512x512"
    );
}

#[rstest]
#[case::jpeg_ext(CONFIG_STYLES.as_str(), "/style/maplibre_demo/0/0/0.jpg")]
#[case::jpeg_zoom_1(CONFIG_STYLES.as_str(), "/style/maplibre_demo/1/0/0.jpg")]
#[tokio::test]
#[tracing_test::traced_test]
async fn render_tile_jpeg(#[case] config: &str, #[case] path: &str) {
    let app = create_app! { config };

    let req = test_get(path).to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;

    assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), "image/jpeg");

    let body = read_body(response).await;
    assert!(
        body.len() > 1000,
        "JPEG should have reasonable size for {path}, got {}",
        body.len()
    );

    // Verify JPEG magic bytes
    assert_eq!(&body[..3], JPEG_MAGIC, "Response is not a valid JPEG");

    // Decode and verify dimensions
    let img = image::load_from_memory_with_format(&body, image::ImageFormat::Jpeg)
        .expect("Failed to decode JPEG response");
    assert_eq!(
        (img.width(), img.height()),
        (512, 512),
        "Tile must be 512x512"
    );
}

#[tokio::test]
async fn render_tile_jpeg_redirects_to_jpg() {
    let app = create_app! { CONFIG_STYLES.as_str() };
    let req = test_get("/style/maplibre_demo/0/0/0.jpeg").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 301);
    assert_eq!(
        resp.headers().get(LOCATION).expect("Location header"),
        "/style/maplibre_demo/0/0/0.jpg",
    );
}

#[tokio::test]
#[tracing_test::traced_test]
async fn render_tile_not_found_style() {
    let app = create_app! { CONFIG_STYLES.as_str() };

    let req = test_get("/style/nonexistent_style/0/0/0.png").to_request();
    let response = call_service(&app, req).await;

    assert_eq!(response.status(), 404);
    let body = String::from_utf8(read_body(response).await.to_vec()).unwrap();
    assert_eq!(body, "No such style exists");
}

#[tokio::test]
#[tracing_test::traced_test]
async fn render_tile_impossible() {
    let app = create_app! { CONFIG_STYLES.as_str() };

    // 4000,4000 is not possible for zoom level 0
    let req = test_get("/style/maplibre_demo/0/4000/4000.png").to_request();
    let response = call_service(&app, req).await;

    assert_eq!(response.status(), 400);
    let body = String::from_utf8(read_body(response).await.to_vec()).unwrap();
    assert_eq!(body, "Invalid tile coordinates for zoom level");
}

#[tokio::test]
#[tracing_test::traced_test]
async fn render_different_tiles_differ() {
    let app = create_app! { CONFIG_STYLES.as_str() };

    let req_a = test_get("/style/maplibre_demo/0/0/0.png").to_request();
    let resp_a = call_service(&app, req_a).await;
    let body_a = read_body(assert_response(resp_a).await).await;

    let req_b = test_get("/style/maplibre_demo/1/1/0.png").to_request();
    let resp_b = call_service(&app, req_b).await;
    let body_b = read_body(assert_response(resp_b).await).await;

    assert_ne!(
        body_a, body_b,
        "Different tile coordinates must produce different images"
    );
}

#[tokio::test]
#[tracing_test::traced_test]
async fn render_concurrent_requests() {
    let app = create_app! { CONFIG_STYLES.as_str() };

    let coords = [
        "/style/maplibre_demo/0/0/0.png",
        "/style/maplibre_demo/1/0/0.png",
        "/style/maplibre_demo/1/1/0.png",
        "/style/maplibre_demo/1/0/1.png",
        "/style/maplibre_demo/1/1/1.png",
    ];

    let futures = coords
        .iter()
        .map(|path| call_service(&app, test_get(path).to_request()));

    let responses = futures::future::join_all(futures).await;

    let mut bodies = Vec::new();
    for (i, response) in responses.into_iter().enumerate() {
        let response = assert_response(response).await;
        assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), "image/png");
        let body = read_body(response).await;
        assert!(
            body.len() > 1000,
            "Concurrent request {i} should produce a valid image"
        );
        assert_eq!(
            &body[..8],
            PNG_MAGIC,
            "Concurrent request {i} is not valid PNG"
        );
        bodies.push(body);
    }

    // Verify not all responses are identical (renderer isn't returning cached static image)
    let unique_count = bodies
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();
    assert!(
        unique_count > 1,
        "All {0} concurrent responses are identical - renderer may be ignoring coordinates",
        bodies.len()
    );
}

const CAMERA_DIR: &str = "../tests/fixtures/static_camera";

async fn png_response(uri: &str) -> Vec<u8> {
    let app = create_app! { CONFIG_STYLES.as_str() };
    let req = test_get(uri).to_request();
    let resp = call_service(&app, req).await;
    let resp = assert_response(resp).await;
    read_body(resp).await.to_vec()
}

async fn jpeg_response(uri: &str) -> Vec<u8> {
    let app = create_app! { CONFIG_STYLES.as_str() };
    let req = test_get(uri).to_request();
    let resp = call_service(&app, req).await;
    let resp = assert_response(resp).await;
    assert_eq!(
        resp.headers().get(CONTENT_TYPE).expect("content-type set"),
        "image/jpeg"
    );
    read_body(resp).await.to_vec()
}

/// Match against a git-tracked reference PNG, or
/// write + pass on first run if the fixture doesn't yet exist.
/// Tolerance is loose (>=0.95) so cross-machine AA jitter doesn't cause flakes.
#[track_caller]
#[expect(clippy::unwrap_used, reason = "test assertion helper")]
fn assert_png_matches(ref_path: &Path, body: &[u8]) {
    let display = ref_path.display();
    assert_eq!(&body[..8], PNG_MAGIC, "{display}: not a valid PNG");
    if !ref_path.exists() {
        std::fs::create_dir_all(ref_path.parent().unwrap()).unwrap();
        std::fs::write(ref_path, body).unwrap();
        return;
    }
    let reference_bytes = std::fs::read(ref_path).unwrap();
    let rendered = image::load_from_memory_with_format(body, image::ImageFormat::Png)
        .expect("rendered is png")
        .to_rgba8();
    let reference = image::load_from_memory_with_format(&reference_bytes, image::ImageFormat::Png)
        .expect("reference is png")
        .to_rgba8();
    assert_eq!(
        (rendered.width(), rendered.height()),
        (reference.width(), reference.height()),
        "{display}: dimensions differ from fixture"
    );
    let similarity = image_compare::rgba_hybrid_compare(&reference, &rendered)
        .expect("image_compare succeeds on equal-sized RGBA images");
    assert!(
        similarity.score >= 0.95,
        "{display}: similarity {:.4} < 0.95 - output drifted from fixture. \
                 If this is intentional, delete the fixture and re-run.",
        similarity.score
    );
}

#[track_caller]
fn assert_visually_distinct(name: &str, a: &[u8], b: &[u8]) {
    let img_a = image::load_from_memory(a).expect("a decodes").to_rgba8();
    let img_b = image::load_from_memory(b).expect("b decodes").to_rgba8();
    let similarity = image_compare::rgba_hybrid_compare(&img_a, &img_b)
        .expect("image_compare succeeds on equal-sized RGBA images");
    assert!(
        similarity.score < 0.9995,
        "{name}: outputs essentially identical (similarity {:.4}) - option had no effect",
        similarity.score
    );
}

#[track_caller]
fn assert_visually_similar(name: &str, a: &[u8], b: &[u8]) {
    let img_a = image::load_from_memory(a).expect("a decodes").to_rgba8();
    let img_b = image::load_from_memory(b).expect("b decodes").to_rgba8();
    assert_eq!(
        (img_a.width(), img_a.height()),
        (img_b.width(), img_b.height()),
        "{name}: dimensions differ",
    );
    let similarity = image_compare::rgba_hybrid_compare(&img_a, &img_b)
        .expect("image_compare succeeds on equal-sized RGBA images");
    assert!(
        similarity.score >= 0.95,
        "{name}: similarity {:.4} < 0.95",
        similarity.score
    );
}

fn camera_ref(stem: &str) -> PathBuf {
    PathBuf::from(CAMERA_DIR).join(format!("{stem}.png"))
}

#[tokio::test]
async fn format_jpeg_returns_jpeg() {
    let body = jpeg_response("/style/maplibre_demo/static/0,0,0/200x200.jpg").await;
    assert_eq!(&body[..3], JPEG_MAGIC);
}

#[tokio::test]
async fn center_z0_matches_fixture() {
    let body = png_response("/style/maplibre_demo/static/0,0,0/200x200.png").await;
    assert_png_matches(&camera_ref("center_z0"), &body);
}

#[tokio::test]
async fn center_z3_differs_from_z0() {
    let z0 = png_response("/style/maplibre_demo/static/0,0,0/200x200.png").await;
    let z3 = png_response("/style/maplibre_demo/static/0,0,3/200x200.png").await;
    assert_png_matches(&camera_ref("center_z3"), &z3);
    assert_visually_distinct("zoom", &z0, &z3);
}

#[tokio::test]
async fn bearing_changes_output() {
    let north_up = png_response("/style/maplibre_demo/static/0,0,2@0/200x200.png").await;
    let rotated = png_response("/style/maplibre_demo/static/0,0,2@90/200x200.png").await;
    assert_png_matches(&camera_ref("bearing_90"), &rotated);
    assert_visually_distinct("bearing", &north_up, &rotated);
}

#[tokio::test]
async fn pitch_changes_output() {
    let flat = png_response("/style/maplibre_demo/static/0,0,2@0,0/200x200.png").await;
    let tilted = png_response("/style/maplibre_demo/static/0,0,2@0,45/200x200.png").await;
    assert_png_matches(&camera_ref("pitch_45"), &tilted);
    assert_visually_distinct("pitch", &flat, &tilted);
}

#[tokio::test]
async fn bbox_framing() {
    let body = png_response("/style/maplibre_demo/static/-30,-30,30,30/200x200.png").await;
    assert_png_matches(&camera_ref("bbox_pm30"), &body);
}

#[tokio::test]
async fn center_off_origin_matches_fixture() {
    let body = png_response("/style/maplibre_demo/static/13.4,52.5,4/200x200.png").await;
    assert_png_matches(&camera_ref("center_berlin_z4"), &body);
    let origin = png_response("/style/maplibre_demo/static/0,0,4/200x200.png").await;
    assert_visually_distinct("off-origin vs origin", &body, &origin);
}

#[tokio::test]
async fn bbox_off_origin_differs_from_origin_bbox() {
    let europe = png_response("/style/maplibre_demo/static/-10,40,30,60/200x200.png").await;
    let origin = png_response("/style/maplibre_demo/static/-20,-10,20,10/200x200.png").await;
    assert_png_matches(&camera_ref("bbox_europe"), &europe);
    assert_visually_distinct("off-origin bbox vs origin bbox", &europe, &origin);
}

#[tokio::test]
async fn bbox_equivalent_to_explicit_center_zoom() {
    let bbox = png_response("/style/maplibre_demo/static/-30,-30,30,30/200x200.png").await;
    let center = png_response("/style/maplibre_demo/static/0,0,2.16/200x200.png").await;
    assert_visually_similar("bbox vs equivalent center+zoom", &bbox, &center);
}

#[tokio::test]
async fn scale_2x_doubles_pixel_dimensions() {
    let one_x = png_response("/style/maplibre_demo/static/0,0,0/100x100.png").await;
    let two_x = png_response("/style/maplibre_demo/static/0,0,0/100x100@2x.png").await;
    let img_1x = image::load_from_memory(&one_x).expect("1x decodes");
    let img_2x = image::load_from_memory(&two_x).expect("2x decodes");
    assert_eq!((img_1x.width(), img_1x.height()), (100, 100));
    assert_eq!((img_2x.width(), img_2x.height()), (200, 200));
}

/// Centered z=2 framing at a fixed 200×200 (for overlay-rendering tests
/// where the camera shouldn't follow the geometry).
const URI_CENTERED_200: &str = "/style/maplibre_demo/static/0,0,2/200x200.png";

const OVERLAY_1X_DIR: &str = "../tests/fixtures/static_overlays/1x";

async fn post_png_body(uri: &str, overlay_body: &[u8]) -> Vec<u8> {
    let app = create_app! { CONFIG_STYLES.as_str() };
    let req = TestRequest::post()
        .uri(uri)
        .insert_header(("content-type", "application/json"))
        .set_payload(overlay_body.to_vec())
        .to_request();
    let resp = call_service(&app, req).await;
    let resp = assert_response(resp).await;
    read_body(resp).await.to_vec()
}

async fn post_no_body(uri: &str) -> Vec<u8> {
    post_png_body(uri, b"").await
}

#[expect(clippy::panic, reason = "test fixture loader")]
async fn run_overlay_scenario(geojson_path: &Path, uri: &str, expected_dir: &str) {
    let display = geojson_path.display();
    let body = std::fs::read(geojson_path).unwrap_or_else(|e| panic!("read {display}: {e}"));
    let png = post_png_body(uri, &body).await;
    let stem = geojson_path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .expect("scenario filename stem");
    let ref_path = PathBuf::from(expected_dir).join(format!("{stem}.png"));
    assert_png_matches(&ref_path, &png);
}

#[tokio::test]
async fn empty_body_renders_base_map() {
    // Same camera as GET; the byte-identical PNG fixture from the GET
    // suite proves the POST path renders the same map when no overlays
    // are supplied.
    let body = post_no_body("/style/maplibre_demo/static/0,0,0/200x200.png").await;
    assert_png_matches(&camera_ref("center_z0"), &body);
}

/// A parsed but empty overlay spec hits a different branch than an empty
/// body: the body is decoded and deserialized into an `OverlaySpec`, but the
/// resulting `OverlaySpec::is_empty()` short-circuits the apply pass so the
/// base map is returned untouched.
#[tokio::test]
async fn empty_overlay_renders_base_map() {
    let body = post_png_body(
        "/style/maplibre_demo/static/0,0,0/200x200.png",
        br#"{"type": "FeatureCollection", "features": []}"#,
    )
    .await;
    assert_png_matches(&camera_ref("center_z0"), &body);
}

test_each_path! {
    #[tokio::test]
    async in "tests/fixtures/static_overlays/input"
    as overlays_1x
    => async |p: &Path| run_overlay_scenario(p, URI_CENTERED_200, OVERLAY_1X_DIR).await
}

/// Same centered framing at @2x. Camera (0,0,2) on 200x200@2x outputs a
/// 400x400 image at scale `256·2²·2 = 2048 px/world`, so any feature off
/// the camera center must scale up linearly with the pixel ratio.
/// `fill_opacity` carries off-center polygons (centered at ±15°)
/// and so locks down the @Nx alignment between overlays and the base map.
const URI_CENTERED_200_2X: &str = "/style/maplibre_demo/static/0,0,2/200x200@2x.png";

const OVERLAY_2X_DIR: &str = "../tests/fixtures/static_overlays/2x";

test_each_path! {
    #[tokio::test]
    async in "tests/fixtures/static_overlays/input"
    as overlays_2x
    => async |p: &Path| run_overlay_scenario(p, URI_CENTERED_200_2X, OVERLAY_2X_DIR).await
}

/// Same overlays, same 200×200 frame, but the camera is tilted 60° (pitch).
/// This locks down that overlays are re-projected through the same view
/// matrix as the base map -- a flat 2D draw over the rendered tile would
/// visibly drift here.
const URI_CENTERED_200_PITCH: &str = "/style/maplibre_demo/static/0,0,2@0,60/200x200.png";

const OVERLAY_1X_PITCH_DIR: &str = "../tests/fixtures/static_overlays/1x_pitch";

test_each_path! {
    #[tokio::test]
    async in "tests/fixtures/static_overlays/input"
    as overlays_1x_pitch
    => async |p: &Path| run_overlay_scenario(p, URI_CENTERED_200_PITCH, OVERLAY_1X_PITCH_DIR).await
}

/// Same overlays, same 200×200 frame, but the camera is rotated 45°
/// (bearing). Isolates the bearing axis from pitch so a regression in
/// rotation handling shows up independently of tilt.
const URI_CENTERED_200_BEARING: &str = "/style/maplibre_demo/static/0,0,2@45/200x200.png";

const OVERLAY_1X_BEARING_DIR: &str = "../tests/fixtures/static_overlays/1x_bearing";

test_each_path! {
    #[tokio::test]
    async in "tests/fixtures/static_overlays/input"
    as overlays_1x_bearing
    => async |p: &Path| run_overlay_scenario(p, URI_CENTERED_200_BEARING, OVERLAY_1X_BEARING_DIR).await
}

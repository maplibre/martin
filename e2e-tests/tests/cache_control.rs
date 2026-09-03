//! The `cache_control` server setting and its per-source overrides.

use martin_e2e_tests::{Martin, StartError, mbtiles_fixture};

const CONFIG: &str = "
cache_control: public, max-age=3600
pmtiles:
  sources:
    pmt: tests/fixtures/pmtiles/png.pmtiles
sprites:
  paths: tests/fixtures/sprites/src1
fonts:
  - tests/fixtures/fonts/overpass-mono-regular.ttf
styles:
  sources:
    maplibre: tests/fixtures/styles/maplibre_demo.json
";

#[tokio::test]
async fn the_configured_cache_control_is_sent_on_every_content_endpoint() {
    let mut martin = Martin::builder()
        .config(CONFIG)
        .start()
        .await
        .expect("failed to start martin");

    for path in [
        "/pmt/0/0/0",
        "/pmt",
        "/catalog",
        "/sprite/src1.json",
        "/font/Overpass Mono Regular/0-255",
        "/style/maplibre",
    ] {
        let response = martin.get(path).await;
        assert_eq!(response.status(), 200, "GET {path}");
        assert_eq!(
            response.header("cache-control"),
            Some("public, max-age=3600"),
            "GET {path} must carry the configured Cache-Control header"
        );
    }

    martin.stop().await;
    martin.assert_startup_warnings();
}

#[tokio::test]
async fn the_health_endpoint_keeps_its_no_cache_policy() {
    let mut martin = Martin::builder()
        .config(CONFIG)
        .start()
        .await
        .expect("failed to start martin");

    let response = martin.get("/health").await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.header("cache-control"), Some("no-cache"));

    martin.stop().await;
    martin.assert_startup_warnings();
}

#[tokio::test]
async fn no_cache_control_is_sent_unless_configured() {
    let mut martin = Martin::builder()
        .config(
            "
pmtiles:
  sources:
    pmt: tests/fixtures/pmtiles/png.pmtiles
",
        )
        .start()
        .await
        .expect("failed to start martin");

    let response = martin.get("/pmt/0/0/0").await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.header("cache-control"), None);

    martin.stop().await;
    martin.assert_startup_warnings();
}

async fn martin_with_per_source_overrides() -> (tempfile::TempDir, Martin) {
    let dir = tempfile::tempdir().expect("failed to create a temp dir");
    let cities = mbtiles_fixture(dir.path(), "world_cities").await;
    let cities = cities.to_str().expect("fixture path is valid utf-8");
    let config = format!(
        "
cache_control: public, max-age=3600
mbtiles:
  sources:
    plain: {cities}
    pinned:
      path: {cities}
      cache_control: no-store
    pinned_alike:
      path: {cities}
      cache_control: no-store
    pinned_differently:
      path: {cities}
      cache_control: public, max-age=60
"
    );
    let martin = Martin::builder()
        .config(&config)
        .start()
        .await
        .expect("failed to start martin");
    (dir, martin)
}

#[tokio::test]
async fn a_per_source_cache_control_overrides_the_server_default() {
    let (_dir, mut martin) = martin_with_per_source_overrides().await;

    let response = martin.get("/pinned/0/0/0").await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.header("cache-control"), Some("no-store"));

    let response = martin.get("/plain/0/0/0").await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.header("cache-control"),
        Some("public, max-age=3600")
    );

    martin.stop().await;
}

#[tokio::test]
async fn a_composite_request_uses_the_override_only_when_all_sources_agree() {
    let (_dir, mut martin) = martin_with_per_source_overrides().await;

    let response = martin.get("/pinned,pinned_alike/0/0/0").await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.header("cache-control"), Some("no-store"));

    let response = martin.get("/pinned,pinned_differently/0/0/0").await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.header("cache-control"),
        Some("public, max-age=3600")
    );

    martin.stop().await;
}

#[tokio::test]
async fn an_invalid_per_source_cache_control_value_fails_startup() {
    let error = Martin::builder()
        .config(
            "
pmtiles:
  sources:
    pmt:
      path: tests/fixtures/pmtiles/png.pmtiles
      cache_control: max-age=invalid
",
        )
        .start()
        .await
        .expect_err("martin must reject an invalid per-source Cache-Control value");
    let StartError::EarlyExit { status, log } = error else {
        panic!("expected an early exit, got: {error}");
    };
    assert!(!status.success(), "exit status must be a failure: {status}");
    assert!(
        log.contains("invalid Cache-Control header value 'max-age=invalid'"),
        "log must name the invalid value; log:\n{log}"
    );
}

#[tokio::test]
async fn an_invalid_cache_control_value_fails_startup() {
    let error = Martin::builder()
        .config("cache_control: max-age=invalid")
        .start()
        .await
        .expect_err("martin must reject an invalid Cache-Control value");
    let StartError::EarlyExit { status, log } = error else {
        panic!("expected an early exit, got: {error}");
    };
    assert!(!status.success(), "exit status must be a failure: {status}");
    assert!(
        log.contains("invalid Cache-Control header value 'max-age=invalid'"),
        "log must name the invalid value; log:\n{log}"
    );
}

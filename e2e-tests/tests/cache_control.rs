//! The `cache_control` server setting.

use martin_e2e_tests::{Martin, StartError};

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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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

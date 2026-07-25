//! Rust port of the "auto configured Martin" pmtiles slice of `tests/test.sh`
//! (the `auto_mini` and `route_prefix_health` groups).

use std::fs;

use martin_integration_tests::Martin;

/// `tests/test.sh` "Test minimum auto configured Martin": start martin with
/// only a pmtiles directory, snapshot the catalog, the response headers, and
/// the `--save-config` output, then check the log.
#[test]
fn auto_configured_minimal() {
    let tmp = tempfile::tempdir().expect("failed to create a temp dir");
    let save_config = tmp.path().join("save_config.yaml");
    let mut martin = Martin::builder()
        .arg("--save-config")
        .arg(&save_config)
        .arg("tests/fixtures/pmtiles2")
        .start();

    let catalog = martin.get("/catalog");
    assert_eq!(catalog.status(), 200);
    insta::assert_snapshot!("catalog_auto_headers", catalog.headers_snapshot());

    // The `rendering` capability flag only exists in Linux builds (see
    // `CatalogSettings`), so assert `settings` per platform and snapshot the
    // portable remainder.
    let mut catalog_json = catalog.json();
    let settings = catalog_json
        .as_object_mut()
        .expect("catalog is a json object")
        .remove("settings")
        .expect("catalog has a settings key");
    #[cfg(target_os = "linux")]
    assert_eq!(settings, serde_json::json!({ "rendering": false }));
    #[cfg(not(target_os = "linux"))]
    assert_eq!(settings, serde_json::json!({}));

    let mut insta_settings = insta::Settings::clone_current();
    insta_settings.set_sort_maps(true);
    insta_settings.bind(|| insta::assert_json_snapshot!("catalog_auto", catalog_json));

    let tile = martin.get("/webp2/0/0/0");
    assert_eq!(tile.status(), 200);
    insta::assert_snapshot!("webp2_0_0_0_headers", tile.headers_snapshot());
    assert!(
        tile.body().starts_with(b"RIFF"),
        "expected a webp (RIFF) tile body"
    );

    let saved = fs::read_to_string(&save_config).expect("martin did not write --save-config");
    insta::assert_snapshot!("save_config", martin.redact(&saved));

    martin.stop();
    martin.assert_log_contains("Defaulting `pmtiles.allow_http` to `true`");
    martin.assert_log_contains("Environment variable AWS_SKIP_CREDENTIALS is deprecated");
    martin.assert_log_contains("Environment variable AWS_REGION is deprecated");
    martin.assert_log_clean();
}

/// `tests/test.sh` "Test route prefix health endpoint availability":
/// with `--route-prefix`, `/health` must stay reachable at the root so that
/// docker healthchecks keep working (maplibre/martin#2723), while the
/// prefixed health endpoint works as well.
#[test]
fn route_prefix_keeps_root_health() {
    let mut martin = Martin::builder()
        .arg("--route-prefix")
        .arg("/foo")
        .arg("tests/fixtures/pmtiles2")
        .readiness_path("/foo/health")
        .start();

    let root = martin.get("/health");
    assert_eq!(root.status(), 200);
    assert_eq!(root.text(), "OK");

    let prefixed = martin.get("/foo/health");
    assert_eq!(prefixed.status(), 200);
    assert_eq!(prefixed.text(), "OK");

    martin.stop();
    martin.assert_log_contains("Defaulting `pmtiles.allow_http` to `true`");
    martin.assert_log_contains("Environment variable AWS_SKIP_CREDENTIALS is deprecated");
    martin.assert_log_contains("Environment variable AWS_REGION is deprecated");
    martin.assert_log_clean();
}

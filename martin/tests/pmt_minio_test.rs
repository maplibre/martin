//! End-to-end test for the [`PMTilesReloader`] remote-polling code path against the public
//! `/catalog` and `/{src}/{z}/{x}/{y}` endpoints.
//!
//! Spins up a MinIO testcontainer (S3-compatible), uploads `.pmtiles` blobs to it, and
//! configures Martin with `paths: [s3://bucket/]`. The reloader's polling loop discovers
//! the blobs and registers them with the live `TileSourceManager`; the test then issues
//! HTTP requests against the in-process actix app and asserts that the catalog reflects
//! adds/removes within a few polling intervals.
//!
//! Requires Docker. Mirrors the `test-pg` pattern: the file is only compiled when the
//! `test-minio` feature is enabled, so the regular `cargo test` suite skips it without
//! needing per-test `#[ignore]` annotations. CI runs it with
//! `cargo test --features test-minio --test pmt_minio_test`.

#![cfg(feature = "test-minio")]

use std::collections::HashMap;
use std::time::Duration;

use actix_web::dev::ServiceResponse;
use actix_web::test::{TestRequest, call_service, init_service, read_body, read_body_json};
use actix_web::web::Data;
use indoc::formatdoc;
use martin::config::file::ProcessConfig;
use martin::config::file::reload::pmtiles::PMTilesReloader;
use martin::config::file::srv::SrvConfig;
use martin::config::primitives::IdResolver;
use object_store::path::Path as ObjPath;
use object_store::{ObjectStore, ObjectStoreExt as _, PutPayload};
use serde_json::Value;
use testcontainers_modules::minio::MinIO;
use testcontainers_modules::testcontainers::ContainerAsync;
use testcontainers_modules::testcontainers::core::{CmdWaitFor, ExecCommand};
use testcontainers_modules::testcontainers::runners::AsyncRunner as _;
use url::Url;

pub mod utils;

const BUCKET: &str = "pmt-bucket";
/// `47 KB` valid PMTiles blob shipped in the test fixtures. Has a single tileset id of
/// "ne2sr" and tiles at z 0–4 in webp format — small enough that uploading it from a
/// `&'static [u8]` payload is fast.
const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/pmtiles/png.pmtiles");

async fn start_minio() -> (ContainerAsync<MinIO>, String) {
    let minio = MinIO::default()
        .start()
        .await
        .expect("MinIO container failed to start (is Docker running?)");
    // MinIO maps subdirectories of /data to buckets, so creating the directory creates
    // the bucket without needing an mc client or signed PUT request.
    minio
        .exec(
            ExecCommand::new(["mkdir", &format!("/data/{BUCKET}")])
                .with_cmd_ready_condition(CmdWaitFor::exit()),
        )
        .await
        .unwrap();
    let host = minio.get_host().await.unwrap();
    let port = minio.get_host_port_ipv4(9000).await.unwrap();
    let endpoint = format!("http://{host}:{port}");
    (minio, endpoint)
}

fn s3_options(endpoint: &str) -> HashMap<String, String> {
    let mut o = HashMap::new();
    o.insert("aws_endpoint".into(), endpoint.to_string());
    o.insert("aws_access_key_id".into(), "minioadmin".into());
    o.insert("aws_secret_access_key".into(), "minioadmin".into());
    o.insert("aws_region".into(), "us-east-1".into());
    o.insert("allow_http".into(), "true".into());
    o.insert("virtual_hosted_style_request".into(), "false".into());
    o
}

async fn upload(
    store: &dyn ObjectStore,
    key: &str,
    bytes: &'static [u8],
) -> object_store::PutResult {
    store
        .put(&ObjPath::from(key), PutPayload::from_static(bytes))
        .await
        .expect("upload should succeed against MinIO")
}

async fn catalog_tiles(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = ServiceResponse,
        Error = actix_web::Error,
    >,
) -> serde_json::Map<String, Value> {
    let req = TestRequest::get().uri("/catalog").to_request();
    let resp = call_service(app, req).await;
    assert!(resp.status().is_success(), "/catalog failed: {resp:?}");
    let body: Value = read_body_json(resp).await;
    body.get("tiles")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

/// Polls `/catalog` until the predicate is true or the deadline is reached. Used to wait
/// for the reloader's polling tick to propagate.
async fn wait_for_catalog<F>(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = ServiceResponse,
        Error = actix_web::Error,
    >,
    deadline: Duration,
    label: &str,
    predicate: F,
) where
    F: Fn(&serde_json::Map<String, Value>) -> bool,
{
    let start = std::time::Instant::now();
    loop {
        let tiles = catalog_tiles(app).await;
        if predicate(&tiles) {
            return;
        }
        assert!(
            start.elapsed() <= deadline,
            "timed out waiting for catalog condition '{label}'; current tiles={tiles:?}"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn pmt_minio_polls_catalog_via_public_api() {
    let (_minio, endpoint) = start_minio().await;
    let options = s3_options(&endpoint);

    // Pre-populate the bucket with one fixture so the very first polling tick — which
    // happens immediately on startup — has something to discover.
    let s3_url: Url = format!("s3://{BUCKET}/").parse().unwrap();
    let (store, _base) = object_store::parse_url_opts(&s3_url, &options).unwrap();
    upload(&*store, "alpha.pmtiles", FIXTURE).await;

    // `reload_interval_secs: 1` keeps the polling cadence tight enough that a 5-second
    // wait_for budget is plenty for any single change to propagate. `skip_signature: false`
    // and the explicit `aws_region` are spelled out so global env vars from `just` (e.g.
    // `AWS_SKIP_CREDENTIALS=1`) don't override our MinIO credentials.
    let yaml = formatdoc! {"
        pmtiles:
          reload_interval_secs: 1
          aws_endpoint: {endpoint}
          aws_access_key_id: minioadmin
          aws_secret_access_key: minioadmin
          aws_region: us-east-1
          skip_signature: false
          allow_http: true
          virtual_hosted_style_request: false
          paths:
            - s3://{BUCKET}/
    "};

    let mut config = utils::mock_cfg(&yaml);
    let resolver = IdResolver::new(&[]);
    let state = config.resolve(&resolver).await.expect("resolve config");

    let reloader = PMTilesReloader::new(
        state.tile_manager.clone(),
        resolver,
        &config.pmtiles,
        &ProcessConfig::default(),
    );
    reloader.start().expect("reloader start");

    let app = init_service(
        actix_web::App::new()
            .app_data(Data::new(
                martin::srv::Catalog::new(
                    #[cfg(any(feature = "sprites", feature = "fonts", feature = "styles"))]
                    &state,
                )
                .unwrap(),
            ))
            .app_data(Data::new(state.tile_manager.clone()))
            .app_data(Data::new(SrvConfig::default()))
            .configure(|c| martin::srv::router(c, &SrvConfig::default())),
    )
    .await;

    // Initial discovery: the alpha blob uploaded above must show up.
    wait_for_catalog(&app, Duration::from_secs(10), "alpha discovered", |t| {
        t.contains_key("alpha")
    })
    .await;

    // Add a second blob; polling should pick it up without restarting Martin.
    upload(&*store, "beta.pmtiles", FIXTURE).await;
    wait_for_catalog(&app, Duration::from_secs(10), "beta discovered", |t| {
        t.contains_key("alpha") && t.contains_key("beta")
    })
    .await;

    // The catalog entry exposes content type derived from the actual blob — proves the
    // reloader instantiated a real `PmtilesSource` and not just a placeholder.
    let tiles = catalog_tiles(&app).await;
    let content_type = tiles
        .get("alpha")
        .and_then(|v| v.get("content_type"))
        .and_then(Value::as_str)
        .expect("alpha source should have a content_type");
    assert_eq!(content_type, "image/png");

    // Tile fetch through the public API: a successful response proves end-to-end
    // wiring, from the polling reloader -> TileSourceManager -> actix router ->
    // PmtilesSource (object_store backend) -> MinIO.
    let tile_resp = call_service(&app, TestRequest::get().uri("/alpha/0/0/0").to_request()).await;
    assert!(
        tile_resp.status().is_success(),
        "tile fetch failed: {tile_resp:?}"
    );
    let body = read_body(tile_resp).await;
    assert!(!body.is_empty(), "tile body should be non-empty");

    // Remove a blob; polling should drop it from the catalog.
    store.delete(&ObjPath::from("beta.pmtiles")).await.unwrap();
    wait_for_catalog(&app, Duration::from_secs(10), "beta removed", |t| {
        !t.contains_key("beta") && t.contains_key("alpha")
    })
    .await;
}

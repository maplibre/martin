#![cfg(feature = "test-minio")]

use std::collections::HashMap;
use std::time::Duration;

use actix_web::dev::ServiceResponse;
use actix_web::test::{TestRequest, call_service, init_service, read_body, read_body_json};
use actix_web::web::Data;
use indoc::formatdoc;
use insta::assert_yaml_snapshot;
use martin::config::file::ProcessConfig;
use martin::config::file::reload::pmtiles::PmtilesReloader;
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
const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/pmtiles/png.pmtiles");

/// Distinct fixture used to overwrite [`FIXTURE`] in place. Its tilejson omits the
/// `name` field, while [`FIXTURE`]'s tilejson reports `name: "ne2sr"`. The contrast
/// makes it unambiguous whether the reloader replaced the source after an overwrite.
const STAMEN_FIXTURE: &[u8] =
    include_bytes!("../../tests/fixtures/pmtiles/stamen_toner__raster_CC-BY+ODbL_z3.pmtiles");

async fn start_minio() -> (ContainerAsync<MinIO>, String) {
    let minio = MinIO::default()
        .start()
        .await
        .expect("MinIO container failed to start (is Docker running?)");
    // MinIO maps subdirectories of `/data` to buckets, so creating the directory is
    // sufficient to provision the bucket without an `mc` client or signed PUT.
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

/// Polls `/catalog` until the predicate returns true or the deadline is reached.
/// Used to await reloader propagation between mutations and assertions.
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

    // Seed the bucket so the first polling tick (fired immediately on startup) has a
    // source to discover.
    let s3_url: Url = format!("s3://{BUCKET}/").parse().unwrap();
    let (store, _base) = object_store::parse_url_opts(&s3_url, &options).unwrap();
    upload(&*store, "alpha.pmtiles", FIXTURE).await;

    // A 1s polling cadence keeps the wait_for budgets comfortably above propagation
    // latency. Credentials and region are spelled out explicitly so they cannot be
    // overridden by ambient `AWS_*` environment variables (notably `AWS_SKIP_CREDENTIALS`,
    // which `just` injects in some test profiles).
    let yaml = formatdoc! {"
        pmtiles:
          reload_interval: 1s
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

    let reloader = PmtilesReloader::new(
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

    // Initial discovery: the seeded blob must surface in the catalog.
    wait_for_catalog(&app, Duration::from_secs(10), "alpha discovered", |t| {
        t.contains_key("alpha")
    })
    .await;

    // Add a second blob; polling must pick it up without a server restart.
    upload(&*store, "beta.pmtiles", FIXTURE).await;
    wait_for_catalog(&app, Duration::from_secs(10), "beta discovered", |t| {
        t.contains_key("alpha") && t.contains_key("beta")
    })
    .await;

    // Snapshot the catalog after both blobs are present. Each entry's `content_type`
    // and `name` are derived from the actual blob, confirming the reloader instantiated
    // real `PmtilesSource` instances rather than placeholders.
    let tiles_after_add = catalog_tiles(&app).await;
    assert_yaml_snapshot!(tiles_after_add, @r"
    alpha:
      content_type: image/png
      name: ne2sr
    beta:
      content_type: image/png
      name: ne2sr
    ");

    // A successful tile fetch through the public API verifies end-to-end wiring across
    // the polling reloader, `TileSourceManager`, the actix router, and `PmtilesSource`
    // backed by MinIO via `object_store`.
    let tile_resp = call_service(&app, TestRequest::get().uri("/alpha/0/0/0").to_request()).await;
    let status = tile_resp.status().as_u16();
    let body = read_body(tile_resp).await;
    assert_yaml_snapshot!(
        serde_json::json!({
            "status": status,
            "body_non_empty": !body.is_empty(),
        }),
        @r"
    body_non_empty: true
    status: 200
    "
    );

    // Remove a blob and confirm polling drops it from the catalog.
    store.delete(&ObjPath::from("beta.pmtiles")).await.unwrap();
    wait_for_catalog(&app, Duration::from_secs(10), "beta removed", |t| {
        !t.contains_key("beta") && t.contains_key("alpha")
    })
    .await;

    let tiles_after_remove = catalog_tiles(&app).await;
    assert_yaml_snapshot!(tiles_after_remove, @r"
    alpha:
      content_type: image/png
      name: ne2sr
    ");
}

/// Snapshots `PmtilesReloader` behavior when a remote blob is overwritten in place.
///
/// `RemoteState::tick` diffs the set of object IDs. An overwrite at an unchanged key
/// produces `prev_ids == next_ids`, so the loop returns early and does not replace
/// the `PmtilesSource`; the catalog continues to report the original blob's metadata.
/// This test pins that behavior. If the reloader gains ETag or last-modified tracking,
/// the snapshot will diff and force an explicit documentation update.
#[actix_rt::test]
#[tracing_test::traced_test]
async fn pmt_minio_in_place_blob_overwrite_keeps_existing_source() {
    let (_minio, endpoint) = start_minio().await;
    let options = s3_options(&endpoint);

    let s3_url: Url = format!("s3://{BUCKET}/").parse().unwrap();
    let (store, _base) = object_store::parse_url_opts(&s3_url, &options).unwrap();
    upload(&*store, "alpha.pmtiles", FIXTURE).await;

    let yaml = formatdoc! {"
        pmtiles:
          reload_interval: 1s
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

    let reloader = PmtilesReloader::new(
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

    // Establish the pre-overwrite baseline: wait until the reloader discovers the
    // original blob and the catalog exposes its `name` field.
    wait_for_catalog(
        &app,
        Duration::from_secs(10),
        "alpha discovered with name=ne2sr",
        |t| {
            t.get("alpha")
                .and_then(|v| v.get("name"))
                .and_then(Value::as_str)
                == Some("ne2sr")
        },
    )
    .await;

    // Overwrite the blob with a fixture whose tilejson lacks a `name` field. Under
    // ETag tracking the reloader would replace the source and the `name` field would
    // disappear from the catalog.
    upload(&*store, "alpha.pmtiles", STAMEN_FIXTURE).await;

    // Sleep through several polling ticks (the interval is 1s) to give the reloader
    // every opportunity to detect the overwrite.
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Snapshot the catalog. The `name` field still reflects the original fixture,
    // proving the in-place overwrite was not detected. Any future change that causes
    // this snapshot to diff must be paired with a documentation update in
    // `docs/content/sources-files.md` (PMTiles Hot Reload).
    let tiles = catalog_tiles(&app).await;
    assert_yaml_snapshot!(tiles, @r"
    alpha:
      content_type: image/png
      name: ne2sr
    ");
}

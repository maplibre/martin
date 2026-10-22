//! COG (`Cloud Optimized GeoTIFF`) sources: discovered in a directory, configured by id, and
//! reloaded while the server runs.
//!
//! Binary has to be built with `cog`.

#![cfg(feature = "test-cog")]

use std::fs;

use image::ImageFormat;
use indoc::{formatdoc, indoc};
use martin_e2e_tests::{
    CogFixture, Martin, PROJECTED_CRS_GEO_KEY, StartError, StaticFiles, WatchedDir, fixture, round_floats, tag,
    temp_dir,
};
use rstest::rstest;
use serde_json::Value;

/// The tilejson of `id`, with the server address redacted and every float rounded to ten digits,
/// which is as far as the extent arithmetic agrees across platforms.
async fn tilejson(martin: &Martin, id: &str) -> Value {
    let response = martin.get(&format!("/{id}")).await;
    assert_eq!(response.status(), 200);
    let mut tilejson = martin.redacted_json(&response);
    round_floats(&mut tilejson);
    tilejson
}

fn assert_remote_reads_use_ranges(
    requests: &str,
    path: &str,
    query: Option<&str>,
    expected_reads: usize,
    ranges: &[&str],
) {
    let path = query.map_or_else(|| path.to_owned(), |query| format!("{path}?{query}"));
    let head = format!("HEAD {path} no range");
    let gets = ranges
        .iter()
        .map(|range| format!("GET {path} {range}"))
        .collect::<Vec<_>>();
    let request_count = requests.lines().count();
    let mut head_count = 0;
    let mut get_counts = vec![0; gets.len()];

    for request in requests.lines() {
        if request == head {
            head_count += 1;
        } else if let Some(index) = gets.iter().position(|expected| request == expected) {
            get_counts[index] += 1;
        }
    }

    assert_eq!(
        head_count + get_counts.iter().sum::<usize>(),
        request_count,
        "unexpected remote request in:\n{requests}"
    );

    assert!(
        (expected_reads..=expected_reads * 2).contains(&head_count),
        "expected one source-open HEAD and at most one reload-seed HEAD per read, got {head_count}"
    );
    assert_eq!(get_counts, vec![expected_reads; ranges.len()]);
}

#[tokio::test]
async fn a_directory_publishes_a_source_per_file() {
    let tmp = tempfile::tempdir().expect("failed to create a temp dir");
    let save_config = tmp.path().join("save_config.yaml");
    let mut martin = Martin::builder()
        .arg("--save-config")
        .arg(&save_config)
        .arg("tests/fixtures/cog")
        .start()
        .await
        .expect("failed to start martin");

    let catalog = martin.get("/catalog").await;
    assert_eq!(catalog.status(), 200);
    insta::with_settings!({sort_maps => true}, {
        insta::assert_json_snapshot!(catalog.json()["tiles"], @r#"
        {
          "usda_naip_128_none_z2": {
            "content_type": "image/png"
          },
          "usda_naip_256_lzw_rgb_z2": {
            "content_type": "image/png"
          },
          "usda_naip_256_lzw_z3": {
            "content_type": "image/png"
          },
          "usda_naip_512_deflate_z2": {
            "content_type": "image/png"
          },
          "usda_naip_512_jpeg_z5": {
            "content_type": "image/jpeg"
          },
          "usda_naip_512_webp_z5": {
            "content_type": "image/webp"
          }
        }
        "#);
    });

    let saved = fs::read_to_string(&save_config).expect("martin did not write --save-config");
    let saved = saved.replace(std::path::MAIN_SEPARATOR, "/");
    insta::assert_snapshot!(saved, @"
    listen_addresses: 127.0.0.1:0
    cog:
      paths:
      - tests/fixtures/cog
      sources:
        usda_naip_128_none_z2: tests/fixtures/cog/usda_naip_128_none_z2.tif
        usda_naip_256_lzw_rgb_z2: tests/fixtures/cog/usda_naip_256_lzw_rgb_z2.tif
        usda_naip_256_lzw_z3: tests/fixtures/cog/usda_naip_256_lzw_z3.tif
        usda_naip_512_deflate_z2: tests/fixtures/cog/usda_naip_512_deflate_z2.tif
        usda_naip_512_jpeg_z5: tests/fixtures/cog/usda_naip_512_jpeg_z5.tif
        usda_naip_512_webp_z5: tests/fixtures/cog/usda_naip_512_webp_z5.tif
    ");

    martin.stop().await;
}

#[tokio::test]
async fn a_file_is_published_under_its_stem() {
    let mut martin = Martin::builder()
        .arg(fixture("cog/usda_naip_512_webp_z5.tif"))
        .start()
        .await
        .expect("failed to start martin");

    insta::assert_json_snapshot!(martin.get("/catalog").await.json()["tiles"], @r#"
    {
      "usda_naip_512_webp_z5": {
        "content_type": "image/webp"
      }
    }
    "#);

    martin.stop().await;
}

#[tokio::test]
async fn a_configured_source_is_published_under_its_configured_id() {
    let mut martin = Martin::builder()
        .config(indoc! {"
            cog:
              sources:
                naip: tests/fixtures/cog/usda_naip_512_webp_z5.tif
        "})
        .start()
        .await
        .expect("failed to start martin");

    insta::assert_json_snapshot!(martin.get("/catalog").await.json()["tiles"], @r#"
    {
      "naip": {
        "content_type": "image/webp"
      }
    }
    "#);
    assert_eq!(martin.get("/naip/13/1334/3042").await.status(), 200);

    martin.stop().await;
}

#[tokio::test]
async fn a_configured_cog_is_read_from_an_s3_compatible_store_using_ranges() {
    let tmp = tempfile::tempdir().expect("failed to create a temp dir");
    let save_config = tmp.path().join("save_config.yaml");
    let key = "cogtest/usda_naip_128_none_z2.tif";
    let statics = StaticFiles::serving(&[(key, fixture("cog/usda_naip_128_none_z2.tif"))]).await;
    let base_url = statics.base_url();
    let mut martin = Martin::builder()
        .arg("--save-config")
        .arg(&save_config)
        .config(&formatdoc! {"
            cog:
              allow_http: true
              aws_endpoint: {base_url}
              aws_region: eu-central-1
              aws_access_key_id: test-key
              aws_secret_access_key: test-secret
              skip_signature: true
              virtual_hosted_style_request: false
              sources:
                remote: s3://cogtest/usda_naip_128_none_z2.tif
                local: tests/fixtures/cog/usda_naip_128_none_z2.tif
        "})
        .start()
        .await
        .expect("failed to start martin with an S3 COG");

    let remote = martin.get("/remote/18/42712/97343").await;
    let local = martin.get("/local/18/42712/97343").await;
    assert_eq!(remote.status(), 200);
    assert_eq!(remote.header("content-type"), local.header("content-type"));
    assert_eq!(remote.body(), local.body());
    martin.stop().await;
    martin.assert_log_contains(
        "Value is ignored in favor of the explicit configuration value ignored.kind=Environment variable ignored.key=AWS_REGION config.key=cog.aws_region",
    );

    let saved = fs::read_to_string(&save_config).expect("martin did not write --save-config");
    let mut parsed = serde_saphyr::from_str::<Value>(&saved).expect("--save-config output is valid YAML");
    assert!(!saved.contains("test-key"));
    assert!(!saved.contains("test-secret"));
    assert_eq!(
        parsed["cog"]["aws_endpoint"].as_str(),
        Some(statics.base_url().as_str())
    );
    parsed["cog"]["aws_endpoint"] = "[ENDPOINT]".into();
    insta::with_settings!({sort_maps => true}, {
        insta::assert_json_snapshot!(parsed["cog"], @r#"
        {
          "allow_http": true,
          "aws_endpoint": "[ENDPOINT]",
          "aws_region": "eu-central-1",
          "skip_signature": true,
          "sources": {
            "local": "tests/fixtures/cog/usda_naip_128_none_z2.tif",
            "remote": "s3://cogtest/usda_naip_128_none_z2.tif"
          },
          "virtual_hosted_style_request": false
        }
        "#);
    });

    let mut restarted = Martin::builder()
        .config(&saved)
        .start()
        .await
        .expect("failed to restart Martin from --save-config output");
    assert_eq!(restarted.get("/remote/18/42712/97343").await.status(), 200);
    restarted.stop().await;
    restarted.assert_log_contains(
        "Value is ignored in favor of the explicit configuration value ignored.kind=Environment variable ignored.key=AWS_REGION config.key=cog.aws_region",
    );

    assert_remote_reads_use_ranges(
        &statics.request_log().await,
        "/cogtest/usda_naip_128_none_z2.tif",
        None,
        2,
        &["bytes=0-32767", "bytes=1284-66819"],
    );
}

#[tokio::test]
async fn a_configured_cog_whose_reads_fail_at_startup_loads_when_they_heal() {
    let key = "cogtest/usda_naip_128_none_z2.tif";
    let statics = StaticFiles::serving_failing_gets(&[(key, fixture("cog/usda_naip_128_none_z2.tif"))]).await;
    let base_url = statics.base_url();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            on_invalid: warn
            cog:
              reload_interval: 500ms
              allow_http: true
              aws_endpoint: {base_url}
              skip_signature: true
              sources:
                remote: s3://cogtest/usda_naip_128_none_z2.tif
        "})
        .start()
        .await
        .expect("failed to start martin with a read-failing remote COG");

    assert!(
        martin.get("/catalog").await.json()["tiles"].get("remote").is_none(),
        "the failed source must not be published"
    );
    tokio::time::sleep(std::time::Duration::from_millis(1_200)).await;
    assert!(
        martin.get("/catalog").await.json()["tiles"].get("remote").is_none(),
        "the failed source must not appear while reads keep failing"
    );

    statics.set_fail_gets(false);
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            if martin.get("/remote/18/42712/97343").await.status() == 200 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    })
    .await
    .expect("the source must load once reads heal");

    martin.stop().await;
    martin.assert_log_contains("Tile source resolution warning");
    let errors = martin.take_log_lines("error=");
    assert!(
        errors.iter().all(|line| {
            line.contains("WARN Skipping addition source.id=remote error=")
                || line == "ERROR error=\"Source remote does not exist\""
        }),
        "unexpected errors: {errors:#?}"
    );
    let skipped = errors
        .iter()
        .find(|line| line.contains("WARN Skipping addition source.id=remote error="))
        .expect("the failed reload must be logged");
    let missing = errors
        .iter()
        .find(|line| line.as_str() == "ERROR error=\"Source remote does not exist\"")
        .expect("the unavailable source request must be logged");
    insta::with_settings!({
        filters => vec![
            (r"http://127\.0\.0\.1:\d+", "http://[STATICS]"),
            (r" in [0-9.]+(?:ns|µs|ms|s) -", " in [TIME] -"),
            (r"(?m) +$", ""),
        ]
    }, {
        insta::assert_snapshot!(format!("{skipped}\n{missing}"), @r#"
         WARN Skipping addition source.id=remote error=Unable to read AmazonS3(cogtest)/usda_naip_128_none_z2.tif: Object at location usda_naip_128_none_z2.tif not found: Error performing GET http://[STATICS]/cogtest/usda_naip_128_none_z2.tif in [TIME] - Server returned non-2xx status code: 404 Not Found:
        ERROR error="Source remote does not exist"
        "#);
    });
}

#[tokio::test]
async fn a_failed_update_is_retried_until_the_replacement_reads() {
    let key = "cogtest/usda_naip_128_none_z2.tif";
    let fixture_path = fixture("cog/usda_naip_128_none_z2.tif");
    let statics = StaticFiles::serving(&[(key, fixture_path.clone())]).await;
    let base_url = statics.base_url();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            on_invalid: warn
            cog:
              reload_interval: 500ms
              allow_http: true
              aws_endpoint: {base_url}
              skip_signature: true
              sources:
                remote: s3://cogtest/usda_naip_128_none_z2.tif
        "})
        .start()
        .await
        .expect("failed to start martin with a polled remote COG");
    let tile_url = "/remote/19/85424/194686";
    assert_eq!(martin.get(tile_url).await.status(), 200);

    statics.set_fail_gets(true);
    statics.replace(key, &fixture("cog/regressions/usda_naip_128_none_sparse.tif"));
    tokio::time::sleep(std::time::Duration::from_millis(2_000)).await;
    assert_eq!(
        martin.get(tile_url).await.status(),
        200,
        "a failed update must keep serving the last good version"
    );

    statics.set_fail_gets(false);
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            if martin.get(tile_url).await.status() == 204 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    })
    .await
    .expect("the failed update must be retried and applied once reads heal");

    martin.stop().await;
    let errors = martin.take_log_lines("error=");
    assert!(
        errors
            .iter()
            .all(|line| line.contains("WARN Skipping update source.id=remote error=")),
        "unexpected errors: {errors:#?}"
    );
    let skipped = errors.first().expect("the failed reload must be logged");
    insta::with_settings!({
        filters => vec![
            (r"http://127\.0\.0\.1:\d+", "http://[STATICS]"),
            (r" in [0-9.]+(?:ns|µs|ms|s) -", " in [TIME] -"),
            (r"(?m) +$", ""),
        ]
    }, {
        insta::assert_snapshot!(skipped, @" WARN Skipping update source.id=remote error=Unable to read AmazonS3(cogtest)/usda_naip_128_none_z2.tif: Object at location usda_naip_128_none_z2.tif not found: Error performing GET http://[STATICS]/cogtest/usda_naip_128_none_z2.tif in [TIME] - Server returned non-2xx status code: 404 Not Found:");
    });
}

#[tokio::test]
async fn a_replaced_remote_cog_is_detected_and_reloaded() {
    let key = "cogtest/usda_naip_128_none_z2.tif";
    let statics = StaticFiles::serving(&[(key, fixture("cog/usda_naip_128_none_z2.tif"))]).await;
    let base_url = statics.base_url();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            cog:
              reload_interval: 1s
              allow_http: true
              aws_endpoint: {base_url}
              skip_signature: true
              sources:
                remote: s3://cogtest/usda_naip_128_none_z2.tif
        "})
        .start()
        .await
        .expect("failed to start martin with a polled remote COG");
    // This tile is present in the original fixture and explicitly sparse in the replacement.
    let tile_url = "/remote/19/85424/194686";
    let original = martin.get(tile_url).await;
    assert_eq!(original.status(), 200);
    assert_ne!(original.body(), b"");

    // While the object is unchanged, each poll costs one `HEAD` and no rebuild: the number of
    // tile `GET`s stays at what the initial load made, while polls keep arriving.
    martin.wait_for_source("remote").await;
    let counts = |log: &str| {
        (
            log.lines().filter(|l| l.starts_with("GET ")).count(),
            log.lines().filter(|l| l.starts_with("HEAD ")).count(),
        )
    };
    let unchanged_log = statics.request_log().await;
    let (gets, heads) = counts(&unchanged_log);
    assert!(heads > 0, "the poller must re-check the object:\n{unchanged_log}");
    tokio::time::sleep(std::time::Duration::from_millis(2_500)).await;
    let idle_log = statics.request_log().await;
    assert_eq!(
        counts(&idle_log).0,
        gets,
        "an unchanged object must not be reloaded:\n{idle_log}"
    );
    assert!(counts(&idle_log).1 > heads, "polling must keep running:\n{idle_log}");

    statics.replace(key, &fixture("cog/regressions/usda_naip_128_none_sparse.tif"));
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            if martin.get(tile_url).await.status() == 204 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    })
    .await
    .expect("the replacement's sparse tile must become observable within the poll window");

    let reloaded = martin.get(tile_url).await;
    assert_eq!(reloaded.status(), 204);
    assert_eq!(reloaded.body(), b"");
    martin.stop().await;
}

#[tokio::test]
async fn a_remote_cog_prefix_is_discovered_and_polled() {
    let first_key = "cogtest/imagery/first.tif";
    let removed_key = "cogtest/imagery/removed.tif";
    let second_key = "cogtest/imagery/second.tiff";
    let original_fixture = fixture("cog/usda_naip_128_none_z2.tif");
    let statics = StaticFiles::serving(&[
        (first_key, original_fixture.clone()),
        (removed_key, original_fixture.clone()),
    ])
    .await;
    let base_url = statics.base_url();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            cog:
              reload_interval: 2s
              allow_http: true
              aws_endpoint: {base_url}
              skip_signature: true
              paths:
                - s3://cogtest/imagery/
        "})
        .start()
        .await
        .expect("failed to start martin with a remote COG prefix");

    martin.wait_for_source("first").await;
    martin.wait_for_source("removed").await;
    let first_tile = "/first/19/85424/194686";
    let original = martin.get(first_tile).await;
    assert_eq!(original.status(), 200);
    assert_ne!(original.body(), b"");

    statics.replace(first_key, &fixture("cog/regressions/usda_naip_128_none_sparse.tif"));
    statics.insert(second_key, &original_fixture);
    statics.remove(removed_key);

    martin.wait_for_source("second").await;
    martin.wait_for_source_removed("removed").await;
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            if martin.get(first_tile).await.status() == 204 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    })
    .await
    .expect("a replaced object under the prefix must reload within the poll window");
    assert_eq!(martin.get("/second/19/85424/194686").await.status(), 200);

    martin.stop().await;
    let list_requests = statics
        .request_log()
        .await
        .lines()
        .filter(|request| {
            request.starts_with("GET /cogtest?")
                && request.contains("list-type=2")
                && request.contains("prefix=imagery")
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(list_requests, @"
    GET /cogtest?list-type=2&prefix=imagery%2F no range
    GET /cogtest?list-type=2&prefix=imagery%2F no range
    ");
}

#[tokio::test]
async fn a_cog_url_is_read_over_http_using_ranges() {
    let tmp = tempfile::tempdir().expect("failed to create a temp dir");
    let save_config = tmp.path().join("save_config.yaml");
    let name = "usda_naip_512_webp_z5.tif";
    let statics =
        StaticFiles::serving_with_query("token=secret-query", &[(name, fixture(&format!("cog/{name}")))]).await;
    let clean_url = statics.url(name);
    let configured_url = format!("{clean_url}?token=secret-query#secret-fragment");
    let mut martin = Martin::builder()
        .arg("--save-config")
        .arg(&save_config)
        .config(&formatdoc! {"
            cog:
              allow_http: true
              sources:
                usda_naip_512_webp_z5: {configured_url}
        "})
        .start()
        .await
        .expect("failed to start martin with an HTTP COG");

    assert_eq!(martin.get("/usda_naip_512_webp_z5/13/1334/3042").await.status(), 200);
    martin.stop().await;

    let saved = fs::read_to_string(&save_config).expect("martin did not write --save-config");
    assert!(!saved.contains("secret-query"));
    assert!(!saved.contains("secret-fragment"));
    insta::with_settings!({filters => vec![(r"http://127\.0\.0\.1:\d+", "http://[STATICS]")]}, {
        insta::assert_snapshot!(saved, @"
        listen_addresses: 127.0.0.1:0
        cog:
          sources:
            usda_naip_512_webp_z5: http://[STATICS]/usda_naip_512_webp_z5.tif
          allow_http: true
        ");
    });

    assert_remote_reads_use_ranges(
        &statics.request_log().await,
        "/usda_naip_512_webp_z5.tif",
        Some("token=secret-query"),
        1,
        &["bytes=0-28219", "bytes=11166-11777"],
    );
}

#[rstest]
#[case::none_128("usda_naip_128_none_z2", 18, 19, 128, "png")]
#[case::lzw_256("usda_naip_256_lzw_z3", 16, 18, 256, "png")]
#[case::deflate_512("usda_naip_512_deflate_z2", 16, 17, 512, "png")]
#[case::jpeg_512("usda_naip_512_jpeg_z5", 13, 17, 512, "jpeg")]
#[case::webp_512("usda_naip_512_webp_z5", 13, 17, 512, "webp")]
#[tokio::test]
async fn the_tilejson_reports_the_zoom_range_each_overview_resolves_to(
    #[case] id: &str,
    #[case] minzoom: u8,
    #[case] maxzoom: u8,
    #[case] tile_size: u32,
    #[case] format: &str,
) {
    let mut martin = Martin::builder()
        .arg("tests/fixtures/cog")
        .start()
        .await
        .expect("failed to start martin");

    let tilejson = tilejson(&martin, id).await;
    assert_eq!(tilejson["minzoom"], minzoom);
    assert_eq!(tilejson["maxzoom"], maxzoom);
    assert_eq!(tilejson["tileSize"], tile_size);
    assert_eq!(tilejson["format"], format);
    assert_eq!(
        tilejson["tiles"][0],
        Value::from(format!("http://[ADDR]/{id}/{{z}}/{{x}}/{{y}}"))
    );

    martin.stop().await;
}

#[tokio::test]
async fn the_tilejson_bounds_are_the_area_the_image_covers() {
    let mut martin = Martin::builder()
        .arg("tests/fixtures/cog")
        .start()
        .await
        .expect("failed to start martin");

    insta::assert_json_snapshot!(tilejson(&martin, "usda_naip_512_webp_z5").await, @r#"
    {
      "bounds": [
        -121.376953125,
        41.9349765005,
        -121.3330078125,
        42.0003251483
      ],
      "center": [
        -121.3549804688,
        41.9676592037,
        15
      ],
      "format": "webp",
      "maxzoom": 17,
      "minzoom": 13,
      "tileSize": 512,
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/usda_naip_512_webp_z5/{z}/{x}/{y}"
      ]
    }
    "#);

    martin.stop().await;
}

#[rstest]
#[case::none_128("usda_naip_128_none_z2/18/42712/97343", "image/png", ImageFormat::Png, 128)]
#[case::lzw_256("usda_naip_256_lzw_z3/16/10677/24336", "image/png", ImageFormat::Png, 256)]
#[case::deflate_512("usda_naip_512_deflate_z2/16/10677/24336", "image/png", ImageFormat::Png, 512)]
#[case::jpeg_512("usda_naip_512_jpeg_z5/13/1334/3042", "image/jpeg", ImageFormat::Jpeg, 512)]
#[case::webp_512("usda_naip_512_webp_z5/13/1334/3042", "image/webp", ImageFormat::WebP, 512)]
#[tokio::test]
async fn every_compression_serves_a_tile_of_the_images_own_format(
    #[case] path: &str,
    #[case] content_type: &str,
    #[case] format: ImageFormat,
    #[case] tile_size: u32,
) {
    let mut martin = Martin::builder()
        .arg("tests/fixtures/cog")
        .start()
        .await
        .expect("failed to start martin");

    let tile = martin.get(&format!("/{path}")).await;
    assert_eq!(tile.status(), 200);
    assert_eq!(tile.header("content-type"), Some(content_type));
    assert_eq!(tile.image_format(), format);
    assert_eq!(tile.image_size(), (tile_size, tile_size));

    martin.stop().await;
}

#[tokio::test]
async fn the_shape_of_a_tile_response() {
    let mut martin = Martin::builder()
        .arg("tests/fixtures/cog")
        .start()
        .await
        .expect("failed to start martin");

    let tile = martin.get("/usda_naip_128_none_z2/18/42712/97343").await;
    insta::assert_snapshot!(tile.headers_snapshot(), @r#"
    content-length: 4762
    content-type: image/png
    etag: "hJserbINZD8vUVjU3HbGow"
    vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);

    martin.stop().await;
}

#[tokio::test]
async fn a_tile_the_image_does_not_cover_is_empty() {
    let mut martin = Martin::builder()
        .arg("tests/fixtures/cog")
        .start()
        .await
        .expect("failed to start martin");

    let tile = martin.get("/usda_naip_128_none_z2/18/0/0").await;
    assert_eq!(tile.status(), 204);
    assert!(tile.body().is_empty(), "an empty tile has no body");

    martin.stop().await;
}

#[rstest]
#[case::below_the_lowest_overview("12/667/1521")]
#[case::above_the_full_resolution_image("18/42704/97344")]
#[tokio::test]
async fn a_zoom_no_overview_resolves_to_is_rejected(#[case] coordinates: &str) {
    let mut martin = Martin::builder()
        .arg("tests/fixtures/cog")
        .start()
        .await
        .expect("failed to start martin");

    let tile = martin.get(&format!("/usda_naip_512_jpeg_z5/{coordinates}")).await;
    assert_eq!(tile.status(), 404);
    let zoom = coordinates.split('/').next().expect("a zoom level");
    let expected = format!("Zoom {zoom} is outside the supported range: usda_naip_512_jpeg_z5 supports zoom 13-17");
    assert_eq!(tile.text(), expected);

    martin.stop().await;
    martin.assert_log_contains(&format!("ERROR error=\"{expected}\""));
}

#[tokio::test]
async fn reload_adds_updates_and_removes_a_source() {
    let watched = WatchedDir::new();
    let mut martin = Martin::builder()
        .arg(watched.dir())
        .start()
        .await
        .expect("failed to start martin");

    assert_eq!(martin.get("/catalog").await.json()["tiles"], serde_json::json!({}));

    watched.install(fixture("cog/usda_naip_128_none_z2.tif"), "usda_naip_128_none_z2.tif");
    martin.wait_for_source("usda_naip_128_none_z2").await;
    insta::assert_json_snapshot!(martin.get("/catalog").await.json()["tiles"], @r#"
    {
      "usda_naip_128_none_z2": {
        "content_type": "image/png"
      }
    }
    "#);
    assert_eq!(martin.get("/usda_naip_128_none_z2/18/42712/97343").await.status(), 200);

    watched.touch("usda_naip_128_none_z2.tif");
    martin
        .wait_for_log("Updated source source.id=usda_naip_128_none_z2")
        .await;

    watched.remove("usda_naip_128_none_z2.tif");
    martin.wait_for_source_removed("usda_naip_128_none_z2").await;
    assert_eq!(martin.get("/usda_naip_128_none_z2/18/42712/97343").await.status(), 404);

    martin.stop().await;
    martin.assert_log_contains("Added source source.id=usda_naip_128_none_z2");
    martin.assert_log_contains("Updated source source.id=usda_naip_128_none_z2");
    martin.assert_log_contains("Removed source source.id=usda_naip_128_none_z2");
    martin.assert_log_contains(r#"ERROR error="Source usda_naip_128_none_z2 does not exist""#);
}

#[tokio::test]
async fn a_file_emptied_while_served_fails_its_uncached_tiles_without_crashing() {
    let tmp = temp_dir();
    let path = tmp.path().join("served.tif");
    fs::copy(fixture("cog/usda_naip_512_webp_z5.tif"), &path).expect("failed to copy the fixture");
    let mut martin = Martin::builder()
        .arg(&path)
        .start()
        .await
        .expect("failed to start martin");
    let cached = martin.get("/served/13/1334/3042").await;
    assert_eq!(cached.status(), 200);

    fs::write(&path, b"").expect("failed to empty the served file");

    assert_eq!(martin.get("/served/13/1334/3042").await.body(), cached.body());
    assert_eq!(martin.get("/served/13/1334/3041").await.status(), 500);
    assert_eq!(martin.get("/health").await.status(), 200);

    martin.stop().await;
    martin.assert_log_contains("failed to fill whole buffer");
}

/// The COG README states the requirements a file has to meet. Each case breaks one of them in a
/// copy of a fixture that otherwise meets them all.
#[rstest]
#[case::compression_must_be_one_martin_can_decode(
    |cog: CogFixture| cog.set_short(0, tag::COMPRESSION, 32773),
    "PackBits compression with 4 RGB samples is not supported"
)]
#[case::the_planar_configuration_must_be_chunky(
    |cog: CogFixture| cog.set_short(0, tag::PLANAR_CONFIGURATION, 2),
    "bands are stored separately (PlanarConfiguration=2)"
)]
#[case::the_projected_crs_must_be_web_mercator(
    |cog: CogFixture| cog.set_geo_key(PROJECTED_CRS_GEO_KEY, 4326),
    "its CRS is EPSG:4326, but only EPSG:3857 (Web Mercator) is supported"
)]
#[case::the_geo_keys_must_be_a_directory_martin_can_walk(
    |cog: CogFixture| cog.set_geo_key_header([1, 1, 0, 0]),
    "it has no projected CRS, but only EPSG:3857 (Web Mercator) is supported"
)]
#[case::the_geo_keys_must_be_present(
    |cog: CogFixture| cog.remove_tag(0, tag::GEO_KEY_DIRECTORY),
    "it has no GeoTIFF GeoKeyDirectory"
)]
#[case::the_pixels_must_be_square(
    |cog: CogFixture| cog.set_double(0, tag::MODEL_PIXEL_SCALE, 1, 99.0),
    "pixels are not square (0.5971642834779395 x 99 m)"
)]
#[case::the_tie_points_must_come_in_sixes(
    |cog: CogFixture| cog.set_count(0, tag::MODEL_TIEPOINT, 5),
    "ModelPixelScale or ModelTiepoint has too few values"
)]
#[case::the_image_must_be_georeferenced_at_all(
    |cog: CogFixture| cog.remove_tag(0, tag::MODEL_PIXEL_SCALE).remove_tag(0, tag::MODEL_TIEPOINT),
    "it has neither ModelPixelScale + ModelTiepoint nor ModelTransformation"
)]
#[case::every_overview_must_land_on_a_web_mercator_zoom(
    |cog: CogFixture| cog
        .set_double(0, tag::MODEL_PIXEL_SCALE, 0, 0.7)
        .set_double(0, tag::MODEL_PIXEL_SCALE, 1, 0.7),
    "image 0 has 0.7 m pixels, which is not a WebMercatorQuad zoom level for 256 px tiles; image 1 has 1.4 m pixels, which is not a WebMercatorQuad zoom level for 256 px tiles; image 2 has 2.8 m pixels, which is not a WebMercatorQuad zoom level for 256 px tiles"
)]
#[case::every_overview_must_be_readable(
    |cog: CogFixture| cog.remove_tag(2, tag::IMAGE_WIDTH),
    "the TIFF parser failed on malformed metadata: image_width not found"
)]
#[case::every_overview_must_use_the_same_tile_size(
    |cog: CogFixture| cog.set_short(2, tag::TILE_WIDTH, 512),
    "image 2 is tiled in 512x256 px tiles, but the full-resolution image is tiled in 256x256 px tiles"
)]
#[case::the_bands_must_be_a_color_type_martin_can_re_encode(
    |cog: CogFixture| cog.set_short(0, tag::PHOTOMETRIC_INTERPRETATION, 0),
    "LZW compression with 4 WhiteIsZero samples is not supported"
)]
#[tokio::test]
async fn a_file_that_breaks_a_requirement_is_rejected(
    #[case] break_it: fn(CogFixture) -> CogFixture,
    #[case] expected: &str,
) {
    let tmp = temp_dir();
    let path = break_it(CogFixture::new("usda_naip_256_lzw_z3")).write_to(tmp.path(), "broken");

    let error = Martin::builder()
        .arg(&path)
        .start()
        .await
        .expect_err("martin must refuse to publish a COG that breaks a requirement");
    let StartError::EarlyExit { status, log } = error else {
        panic!("expected an early exit, got: {error}");
    };
    assert!(!status.success(), "exit status must be a failure: {status}");
    let path = path.display().to_string();
    let warnings: Vec<&str> = log
        .lines()
        .filter(|line| line.contains("Tile source resolution warning"))
        .collect();
    assert_eq!(
        warnings,
        [format!(
            "ERROR resolve:handle_tile_warnings: Tile source resolution warning warning=Path {path}: {path} cannot be read as tiles: {expected} warnings.count=1"
        )]
    );
}

/// Only the values Martin uses must be present: the pixel scale's X and Y, and the first two rows
/// of the transformation matrix.
#[rstest]
#[case::a_pixel_scale_without_a_z_scale(
    |cog: CogFixture| cog.set_count(0, tag::MODEL_PIXEL_SCALE, 2)
)]
#[case::a_transformation_matrix_without_its_last_row(
    |cog: CogFixture| cog.into_model_transformation().set_count(0, tag::MODEL_TRANSFORMATION, 12)
)]
#[tokio::test]
async fn a_georeference_without_its_unused_values_is_accepted(#[case] trim_it: fn(CogFixture) -> CogFixture) {
    let tmp = temp_dir();
    let path = trim_it(CogFixture::new("usda_naip_256_lzw_z3")).write_to(tmp.path(), "trimmed");
    let mut martin = Martin::builder()
        .arg(&path)
        .start()
        .await
        .expect("failed to start martin");

    assert_eq!(martin.get("/trimmed/16/10677/24336").await.status(), 200);

    martin.stop().await;
}

#[tokio::test]
async fn a_tiff_stored_in_compressed_strips_is_rejected() {
    let path = fixture("files/lzw_strips.tif");
    let error = Martin::builder()
        .arg(&path)
        .start()
        .await
        .expect_err("martin must refuse to publish a TIFF in compressed strips");
    let StartError::EarlyExit { log, .. } = error else {
        panic!("expected an early exit, got: {error}");
    };
    let path = regex::escape(&path.display().to_string());
    insta::with_settings!({filters => vec![
        (path.as_str(), "[PATH]"),
        (r"martin.version=\S+", "martin.version=[VERSION]"),
    ]}, {
        insta::assert_snapshot!(log, @"
         INFO Starting Martin martin.version=[VERSION]
         INFO Config file is not specified, auto-detecting sources
        ERROR resolve:handle_tile_warnings: Tile source resolution warning warning=Path [PATH]: [PATH] cannot be read as tiles: image 0 uses LZW compressed strips, only uncompressed strips are supported warnings.count=1
        ERROR resolve:handle_tile_warnings: error=TileResolutionWarningsIssued warnings.count=1
        ERROR resolve: error=Config(TileResolutionWarningsIssued)
        ERROR warnings issued during tile source resolution
        ");
    });
}

#[tokio::test]
async fn a_file_that_is_not_a_tiff_is_rejected() {
    let tmp = temp_dir();
    let path = tmp.path().join("not_a_tiff.tif");
    fs::write(&path, b"this is not a tiff").expect("failed to write the file");

    let error = Martin::builder()
        .arg(&path)
        .start()
        .await
        .expect_err("martin must refuse to publish a file that is not a TIFF");
    let StartError::EarlyExit { log, .. } = error else {
        panic!("expected an early exit, got: {error}");
    };
    let path = regex::escape(&path.display().to_string());
    insta::with_settings!({filters => vec![
        (path.as_str(), "[PATH]"),
        (r"martin.version=\S+", "martin.version=[VERSION]"),
    ]}, {
        insta::assert_snapshot!(log, @r#"
         INFO Starting Martin martin.version=[VERSION]
         INFO Config file is not specified, auto-detecting sources
        ERROR resolve:handle_tile_warnings: Tile source resolution warning warning=Path [PATH]: Unable to decode TIFF [PATH]: General error: unexpected magic bytes b"th" warnings.count=1
        ERROR resolve:handle_tile_warnings: error=TileResolutionWarningsIssued warnings.count=1
        ERROR resolve: error=Config(TileResolutionWarningsIssued)
        ERROR warnings issued during tile source resolution
        "#);
    });
}

/// A north-up image may state its georeferencing as a transformation matrix instead of a pixel
/// scale and tie point. No checked-in fixture does, so this rewrites one that does not.
#[tokio::test]
async fn a_transformation_matrix_georeferences_an_image_like_a_pixel_scale_and_tie_point() {
    let tmp = temp_dir();
    let path = CogFixture::new("usda_naip_256_lzw_z3")
        .into_model_transformation()
        .write_to(tmp.path(), "usda_naip_256_lzw_z3");
    let mut martin = Martin::builder()
        .arg(&path)
        .start()
        .await
        .expect("failed to start martin");
    let mut with_the_cog_dir = Martin::builder()
        .arg("tests/fixtures/cog")
        .start()
        .await
        .expect("failed to start martin");

    assert_eq!(
        tilejson(&martin, "usda_naip_256_lzw_z3").await,
        tilejson(&with_the_cog_dir, "usda_naip_256_lzw_z3").await
    );
    let tile = martin.get("/usda_naip_256_lzw_z3/16/10677/24336").await;
    assert_eq!(tile.status(), 200);
    assert_eq!(tile.image_size(), (256, 256));

    with_the_cog_dir.stop().await;
    martin.stop().await;
}

#[tokio::test]
async fn an_image_without_an_alpha_band_serves_a_tile_without_one() {
    let mut martin = Martin::builder()
        .arg("tests/fixtures/cog")
        .start()
        .await
        .expect("failed to start martin");

    let tile = martin.get("/usda_naip_256_lzw_rgb_z2/18/42709/97344").await;
    assert_eq!(tile.status(), 200);
    assert_eq!(tile.header("content-type"), Some("image/png"));
    assert_eq!(tile.image_size(), (256, 256));
    assert_eq!(tile.image_color(), image::ColorType::Rgb8);

    martin.stop().await;
}

/// A COG may leave a tile out of the file rather than store a blank one. This fixture stores only
/// tiles 10-11 of rows 14-16, so its very first tile is one of the gaps.
#[tokio::test]
async fn a_tile_the_image_leaves_out_is_empty() {
    let mut martin = Martin::builder()
        .arg("tests/fixtures/cog")
        .start()
        .await
        .expect("failed to start martin");

    let stored = martin.get("/usda_naip_512_jpeg_z5/17/21354/48670").await;
    assert_eq!(stored.status(), 200);

    let left_out = martin.get("/usda_naip_512_jpeg_z5/17/21344/48656").await;
    assert_eq!(left_out.status(), 204);
    assert!(left_out.body().is_empty(), "a tile left out has no body");

    martin.stop().await;
}

/// The mosaic fixtures are the four z16 children of z15 tile 16374/10897, one file each.
#[tokio::test]
async fn a_mosaic_serves_each_tile_from_the_file_its_key_names() {
    let mut martin = Martin::builder()
        .config(indoc! {"
            cog:
              mosaics:
                ortho:
                  path: tests/fixtures/cog/mosaic/{quadkey}.tif
                  zoom: 16
        "})
        .start()
        .await
        .expect("failed to start martin with a mosaic");

    insta::assert_json_snapshot!(tilejson(&martin, "ortho").await, @r#"
    {
      "format": "png",
      "maxzoom": 17,
      "minzoom": 16,
      "tileSize": 256,
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/ortho/{z}/{x}/{y}"
      ]
    }
    "#);
    for path in ["16/32748/21794", "16/32749/21795", "17/65499/43590"] {
        let tile = martin.get(&format!("/ortho/{path}")).await;
        assert_eq!(tile.status(), 200, "{path}");
        assert_eq!(tile.header("content-type"), Some("image/png"));
        assert_eq!(tile.image_size(), (256, 256));
    }
    // No file holds this z16 tile.
    assert_eq!(martin.get("/ortho/16/32750/21794").await.status(), 204);
    // Above the file zoom, no single file holds the tile.
    assert_eq!(martin.get("/ortho/15/16374/10897").await.status(), 404);

    martin.stop().await;
    martin.assert_log_contains(r#"ERROR error="Zoom 15 is outside the supported range: ortho supports zoom 16-17""#);
}

#[tokio::test]
async fn a_mosaic_answers_with_its_cache_control() {
    let mut martin = Martin::builder()
        .config(indoc! {"
            cog:
              mosaics:
                ortho:
                  path: tests/fixtures/cog/mosaic/{quadkey}.tif
                  zoom: 16
                  cache_control: max-age=1234
        "})
        .start()
        .await
        .expect("failed to start martin with a mosaic");

    let tile = martin.get("/ortho/16/32748/21794").await;
    assert_eq!(tile.status(), 200);
    assert_eq!(tile.header("cache-control"), Some("max-age=1234"));

    martin.stop().await;
}

#[tokio::test]
async fn a_mosaic_in_an_s3_compatible_store_matches_the_local_one() {
    let tmp = tempfile::tempdir().expect("failed to create a temp dir");
    let save_config = tmp.path().join("save_config.yaml");
    let files = fs::read_dir(fixture("cog/mosaic"))
        .expect("failed to list the mosaic fixtures")
        .map(|entry| {
            let path = entry.expect("a mosaic fixture").path();
            let name = path.file_stem().unwrap().to_string_lossy().to_string();
            (format!("cogtest/imagery/{name}.tif"), path)
        })
        .collect::<Vec<_>>();
    let files = files
        .iter()
        .map(|(key, path)| (key.as_str(), path.clone()))
        .collect::<Vec<_>>();
    let statics = StaticFiles::serving(&files).await;
    let base_url = statics.base_url();
    let mut martin = Martin::builder()
        .arg("--save-config")
        .arg(&save_config)
        .config(&formatdoc! {"
            cog:
              allow_http: true
              aws_endpoint: {base_url}
              aws_region: eu-central-1
              skip_signature: true
              virtual_hosted_style_request: false
              mosaics:
                remote:
                  path: s3://cogtest/imagery/{{quadkey}}.tif
                  zoom: 16
                  maxzoom: 18
                local:
                  path: tests/fixtures/cog/mosaic/{{quadkey}}.tif
                  zoom: 16
        "})
        .start()
        .await
        .expect("failed to start martin with an S3 mosaic");

    let remote = tilejson(&martin, "remote").await;
    assert_eq!(remote["minzoom"], 16);
    assert_eq!(remote["maxzoom"], 18);
    assert_eq!(remote["format"], "png");
    for path in ["16/32748/21794", "17/65497/43591"] {
        let remote = martin.get(&format!("/remote/{path}")).await;
        let local = martin.get(&format!("/local/{path}")).await;
        assert_eq!(remote.status(), 200, "{path}");
        assert_eq!(remote.body(), local.body(), "{path}");
    }
    assert_eq!(martin.get("/remote/16/32750/21794").await.status(), 204);
    assert_eq!(martin.get("/remote/18/130992/87176").await.status(), 204);
    martin.stop().await;
    martin.assert_log_contains(
        "Value is ignored in favor of the explicit configuration value ignored.kind=Environment variable ignored.key=AWS_REGION config.key=cog.aws_region",
    );

    let saved = fs::read_to_string(&save_config).expect("martin did not write --save-config");
    let parsed = serde_saphyr::from_str::<Value>(&saved).expect("--save-config output is valid YAML");
    insta::assert_json_snapshot!(parsed["cog"]["mosaics"], @r#"
    {
      "local": {
        "path": "tests/fixtures/cog/mosaic/{quadkey}.tif",
        "zoom": 16
      },
      "remote": {
        "maxzoom": 18,
        "path": "s3://cogtest/imagery/{quadkey}.tif",
        "zoom": 16
      }
    }
    "#);
}

/// An overview half a tile off the tile grid is skipped with a warning; the aligned full-resolution
/// image is still served. This puts the fixture back where it was before its overview was aligned.
#[tokio::test]
async fn a_misaligned_overview_is_skipped_with_a_warning() {
    let tmp = temp_dir();
    let path = CogFixture::new("usda_naip_128_none_z2")
        .set_double(0, tag::MODEL_TIEPOINT, 4, 5_156_365.491_089_705)
        .write_to(tmp.path(), "misaligned");
    let mut martin = Martin::builder()
        .arg(&path)
        .start()
        .await
        .expect("failed to start martin");

    let tilejson = tilejson(&martin, "misaligned").await;
    assert_eq!(tilejson["minzoom"], 19);
    assert_eq!(tilejson["maxzoom"], 19);
    assert_eq!(martin.get("/misaligned/19/85424/194685").await.status(), 200);

    martin.stop().await;
    let skipped = martin
        .take_log_lines("Skipping a COG image")
        .join("\n")
        .replace(&format!("{:?}", path.display().to_string()), "[PATH]");
    insta::assert_snapshot!(skipped, @" WARN resolve:resolve_tile_sources: Skipping a COG image that cannot be served cog.location=[PATH] reason=image 1 is not aligned to the zoom 18 tile grid");
}

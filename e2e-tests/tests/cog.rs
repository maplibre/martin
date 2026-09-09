//! COG (`Cloud Optimized GeoTIFF`) sources: discovered in a directory, configured by id, and
//! reloaded while the server runs.
//!
//! Binary has to be built with `unstable-cog`.

#![cfg(feature = "test-cog")]

use std::fs;

use image::ImageFormat;
use martin_e2e_tests::{Martin, StaticFiles, WatchedDir, fixture, round_floats};
use rstest::rstest;
use serde_json::Value;

async fn martin_with_the_cog_dir() -> Martin {
    Martin::builder()
        .arg("tests/fixtures/cog")
        .start()
        .await
        .expect("failed to start martin")
}

/// The tilejson of `id`, with the server address redacted and every float rounded to ten digits,
/// which is as far as the extent arithmetic agrees across platforms.
async fn tilejson(martin: &Martin, id: &str) -> Value {
    let response = martin.get(&format!("/{id}")).await;
    assert_eq!(response.status(), 200);
    let mut tilejson = serde_json::from_str::<Value>(&martin.redact(&response.text()))
        .expect("tilejson is not valid json");
    round_floats(&mut tilejson);
    tilejson
}

fn assert_remote_reads_use_ranges(
    requests: &str,
    path: &str,
    expected_reads: usize,
    ranges: &[&str],
) {
    let head = format!("HEAD {path} no range");
    let gets = ranges
        .iter()
        .map(|range| format!("GET {path} {range}"))
        .collect::<Vec<_>>();
    let request_count = requests.lines().count();
    let mut head_count = 0;
    let mut get_counts = vec![0; gets.len()];

    for request in requests.lines() {
        // Strip a query a URL may carry (only for asserting reads, which the caller checks
        // separately): `METHOD /path[?query] range` loses its `?query` slice, keeping path and
        // range for comparison.
        let request = match request.split_once('?') {
            Some((before, rest)) => {
                let range_tail = rest.split_once(' ').map_or(rest, |pair| pair.1);
                format!("{before} {range_tail}")
            }
            None => request.to_owned(),
        };
        if request == head {
            head_count += 1;
        } else if let Some(index) = gets.iter().position(|expected| request.as_str() == expected.as_str()) {
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
      paths: tests/fixtures/cog
      sources:
        usda_naip_128_none_z2: tests/fixtures/cog/usda_naip_128_none_z2.tif
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
        .config(
            "\
cog:
  sources:
    naip: tests/fixtures/cog/usda_naip_512_webp_z5.tif
",
        )
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
    let mut martin = Martin::builder()
        .arg("--save-config")
        .arg(&save_config)
        .config(&format!(
            "\
cog:
  allow_http: true
  aws_endpoint: {}
  aws_region: eu-central-1
  aws_access_key_id: test-key
  aws_secret_access_key: test-secret
  skip_signature: true
  virtual_hosted_style_request: false
  sources:
    remote: s3://cogtest/usda_naip_128_none_z2.tif
    local: tests/fixtures/cog/usda_naip_128_none_z2.tif
",
            statics.base_url()
        ))
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
        "Environment variable AWS_REGION is ignored in favor of the new configuration value cog.aws_region.",
    );

    let saved = fs::read_to_string(&save_config).expect("martin did not write --save-config");
    let mut parsed =
        serde_saphyr::from_str::<Value>(&saved).expect("--save-config output is valid YAML");
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
        "Environment variable AWS_REGION is ignored in favor of the new configuration value cog.aws_region.",
    );

    assert_remote_reads_use_ranges(
        &statics.request_log().await,
        "/cogtest/usda_naip_128_none_z2.tif",
        2,
        &["bytes=0-32767", "bytes=1284-66819"],
    );
}

/// A configured COG whose object is served but whose range reads fail at startup must load the
/// moment reads heal, even though the object's version never changes. This is the P1 regression
/// for "warn-policy sinks advance the reload baseline past a source they skipped".
#[tokio::test]
async fn a_configured_cog_whose_reads_fail_at_startup_loads_when_they_heal() {
    let key = "cogtest/usda_naip_128_none_z2.tif";
    let statics = StaticFiles::serving_failing_gets(&[(
            key,
            fixture("cog/usda_naip_128_none_z2.tif"),
        )])
        .await;
    let mut martin = Martin::builder()
        .config(&format!(
            "\
on_invalid: warn
cog:
  reload_interval: 500ms
  allow_http: true
  aws_endpoint: {}
  skip_signature: true
  sources:
    remote: s3://cogtest/usda_naip_128_none_z2.tif
",
            statics.base_url()
        ))
        .start()
        .await
        .expect("failed to start martin with a read-failing remote COG");

    // Reads fail, so the object must not be served; the source is absent, not stale.
    let catalog = async || martin.get("/catalog").await.json()["tiles"].get("remote").is_some();
    assert!(!catalog().await, "the failed source must not be published");
    tokio::time::sleep(std::time::Duration::from_millis(1_200)).await;
    assert!(!catalog().await, "the failed source must not appear while reads keep failing");
    // Two poll cycles have now run against the failing object.

    // Heal the object: the unchanged ETag must not matter, the next poll retries the build.
    statics.set_fail_gets(false);
    let tile_url = "/remote/18/42712/97343";
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            if martin.get(tile_url).await.status() == 200 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    })
    .await
    .expect("the source must load once reads heal");

    // The failure was expected; consume the warnings the harness would otherwise flag. Every
    // failure line carries `error=`, regardless of the shape the log takes.
    martin.assert_log_contains("Tile source resolution warning");
    let drained = martin.take_log_lines("error=");
    assert!(!drained.is_empty(), "the failing reads must have been logged");

    martin.stop().await;
}

/// A source that is live-replaced while its reads are failing must be retried on the next poll
/// rather than stuck at its old version: an update failure has to hold the baseline entry back.
#[tokio::test]
async fn a_failed_update_is_retried_until_the_replacement_reads() {
    let key = "cogtest/usda_naip_128_none_z2.tif";
    let fixture_path = fixture("cog/usda_naip_128_none_z2.tif");
    let statics = StaticFiles::serving(&[(key, fixture_path.clone())]).await;
    let mut martin = Martin::builder()
        .config(&format!(
            "\
on_invalid: warn
cog:
  reload_interval: 500ms
  allow_http: true
  aws_endpoint: {}
  skip_signature: true
  sources:
    remote: s3://cogtest/usda_naip_128_none_z2.tif
",
            statics.base_url()
        ))
        .start()
        .await
        .expect("failed to start martin with a polled remote COG");
    let tile_url = "/remote/19/85424/194685";
    assert_eq!(martin.get(tile_url).await.status(), 200);

    // Replace the object with one that is present in the original but sparse in the replacement,
    // while every read of it fails. The gate answers 404 so a failing build errors immediately;
    // two poll cycles later the skip is guaranteed to have been recorded.
    statics.set_fail_gets(true);
    statics.replace(key, &fixture("cog/regressions/usda_naip_128_none_sparse.tif"));
    tokio::time::sleep(std::time::Duration::from_millis(2_000)).await;
    assert_eq!(
        martin.get(tile_url).await.status(),
        200,
        "a failed update must keep serving the last good version"
    );

    // Heal the reads: the replacement must be picked up although its version never changed.
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

    // The failure was expected; consume the warnings the harness would otherwise flag. Every
    // failure line carries `error=`, regardless of the shape the log takes.
    let drained = martin.take_log_lines("error=");
    assert!(!drained.is_empty(), "the failing reads must have been logged");

    martin.stop().await;
}

#[tokio::test]
async fn a_replaced_remote_cog_is_detected_and_reloaded() {
    let key = "cogtest/usda_naip_128_none_z2.tif";
    let statics = StaticFiles::serving(&[(key, fixture("cog/usda_naip_128_none_z2.tif"))]).await;
    let mut martin = Martin::builder()
        .config(&format!(
            "\
cog:
  reload_interval: 1s
  allow_http: true
  aws_endpoint: {}
  skip_signature: true
  sources:
    remote: s3://cogtest/usda_naip_128_none_z2.tif
",
            statics.base_url()
        ))
        .start()
        .await
        .expect("failed to start martin with a polled remote COG");
    // This tile is present in the original fixture and explicitly sparse in the replacement.
    let tile_url = "/remote/19/85424/194685";
    let original = martin.get(tile_url).await;
    assert_eq!(original.status(), 200);
    assert!(!original.body().is_empty());

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
    assert!(
        heads > 0,
        "the poller must re-check the object:\n{unchanged_log}"
    );
    tokio::time::sleep(std::time::Duration::from_millis(2_500)).await;
    let idle_log = statics.request_log().await;
    assert_eq!(
        counts(&idle_log).0,
        gets,
        "an unchanged object must not be reloaded:\n{idle_log}"
    );
    assert!(
        counts(&idle_log).1 > heads,
        "polling must keep running:\n{idle_log}"
    );

    statics.replace(
        key,
        &fixture("cog/regressions/usda_naip_128_none_sparse.tif"),
    );
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
    assert!(reloaded.body().is_empty());
    martin.stop().await;
}

#[tokio::test]
async fn a_remote_cog_prefix_is_discovered_and_polled() {
    let first_key = "cogtest/imagery/first.tif";
    let second_key = "cogtest/imagery/second.tiff";
    let original_fixture = fixture("cog/usda_naip_128_none_z2.tif");
    let statics = StaticFiles::serving(&[(first_key, original_fixture.clone())]).await;
    let mut martin = Martin::builder()
        .config(&format!(
            "\
cog:
  reload_interval: 1s
  allow_http: true
  aws_endpoint: {}
  skip_signature: true
  paths:
    - s3://cogtest/imagery/
",
            statics.base_url()
        ))
        .start()
        .await
        .expect("failed to start martin with a remote COG prefix");

    martin.wait_for_source("first").await;
    let first_tile = "/first/19/85424/194685";
    let original = martin.get(first_tile).await;
    assert_eq!(original.status(), 200);
    assert!(!original.body().is_empty());

    statics.replace(
        first_key,
        &fixture("cog/regressions/usda_naip_128_none_sparse.tif"),
    );
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

    statics.insert(second_key, &original_fixture);
    martin.wait_for_source("second").await;
    assert_eq!(martin.get("/second/19/85424/194685").await.status(), 200);

    statics.remove(first_key);
    martin.wait_for_source_removed("first").await;
    martin.stop().await;

    let requests = statics.request_log().await;
    let list_count = requests
        .lines()
        .filter(|request| {
            request.starts_with("GET /cogtest?")
                && request.contains("list-type=2")
                && request.contains("prefix=imagery")
        })
        .count();
    assert!(
        list_count >= 4,
        "the prefix must be re-listed for each observed change:\n{requests}"
    );
}

#[tokio::test]
async fn a_cog_url_is_read_over_http_using_ranges() {
    let tmp = tempfile::tempdir().expect("failed to create a temp dir");
    let save_config = tmp.path().join("save_config.yaml");
    let name = "usda_naip_512_webp_z5.tif";
    let statics = StaticFiles::serving_with_query(
        "token=secret-query",
        &[(name, fixture(&format!("cog/{name}")))],
    )
    .await;
    let clean_url = statics.url(name);
    let configured_url = format!("{clean_url}?token=secret-query#secret-fragment");
    let mut martin = Martin::builder()
        .arg("--save-config")
        .arg(&save_config)
        .arg(configured_url)
        .start()
        .await
        .expect("failed to start martin with an HTTP COG");

    assert_eq!(
        martin
            .get("/usda_naip_512_webp_z5/13/1334/3042")
            .await
            .status(),
        200
    );
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
        ");
    });

    assert_remote_reads_use_ranges(
        &statics.request_log().await,
        "/usda_naip_512_webp_z5.tif",
        1,
        &["bytes=0-28219", "bytes=11166-11777"],
    );

    // Every request must have carried the query token; the server 403s otherwise, so serving
    // the tile already proves the query was forwarded, and the log makes it checkable.
    let requests = statics.request_log().await;
    assert!(
        requests.lines().all(|line| line.contains("?token=secret-query")),
        "every remote request must carry the configured query:\n{requests}"
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
    let mut martin = martin_with_the_cog_dir().await;

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
    let mut martin = martin_with_the_cog_dir().await;

    insta::assert_json_snapshot!(tilejson(&martin, "usda_naip_512_webp_z5").await, @r#"
    {
      "bounds": [
        -121.376953125,
        41.9349765005,
        -121.3330078125,
        42.0003251483
      ],
      "center": [
        -121.3549804687,
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
#[case::none_128(
    "usda_naip_128_none_z2/18/42712/97343",
    "image/png",
    ImageFormat::Png,
    128
)]
#[case::lzw_256(
    "usda_naip_256_lzw_z3/16/10677/24336",
    "image/png",
    ImageFormat::Png,
    256
)]
#[case::deflate_512(
    "usda_naip_512_deflate_z2/16/10677/24336",
    "image/png",
    ImageFormat::Png,
    512
)]
#[case::jpeg_512(
    "usda_naip_512_jpeg_z5/13/1334/3042",
    "image/jpeg",
    ImageFormat::Jpeg,
    512
)]
#[case::webp_512(
    "usda_naip_512_webp_z5/13/1334/3042",
    "image/webp",
    ImageFormat::WebP,
    512
)]
#[tokio::test]
async fn every_compression_serves_a_tile_of_the_images_own_format(
    #[case] path: &str,
    #[case] content_type: &str,
    #[case] format: ImageFormat,
    #[case] tile_size: u32,
) {
    let mut martin = martin_with_the_cog_dir().await;

    let tile = martin.get(&format!("/{path}")).await;
    assert_eq!(tile.status(), 200);
    assert_eq!(tile.header("content-type"), Some(content_type));
    assert_eq!(tile.image_format(), format);
    assert_eq!(tile.image_size(), (tile_size, tile_size));

    martin.stop().await;
}

#[tokio::test]
async fn the_shape_of_a_tile_response() {
    let mut martin = martin_with_the_cog_dir().await;

    let tile = martin.get("/usda_naip_128_none_z2/18/42712/97343").await;
    insta::assert_snapshot!(tile.headers_snapshot(), @r#"
    content-length: 5132
    content-type: image/png
    etag: "stlGnHweWV6g4Lm4HSG0QA"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);

    martin.stop().await;
}

#[tokio::test]
async fn a_tile_the_image_does_not_cover_is_empty() {
    let mut martin = martin_with_the_cog_dir().await;

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
    let mut martin = martin_with_the_cog_dir().await;

    let tile = martin
        .get(&format!("/usda_naip_512_jpeg_z5/{coordinates}"))
        .await;
    assert_eq!(tile.status(), 404);
    let zoom = coordinates.split('/').next().expect("a zoom level");
    let expected = format!(
        "Zoom {zoom} is outside the supported range: usda_naip_512_jpeg_z5 supports zoom 13-17"
    );
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

    assert_eq!(
        martin.get("/catalog").await.json()["tiles"],
        serde_json::json!({})
    );

    watched.install(
        fixture("cog/usda_naip_128_none_z2.tif"),
        "usda_naip_128_none_z2.tif",
    );
    martin.wait_for_source("usda_naip_128_none_z2").await;
    insta::assert_json_snapshot!(martin.get("/catalog").await.json()["tiles"], @r#"
    {
      "usda_naip_128_none_z2": {
        "content_type": "image/png"
      }
    }
    "#);
    assert_eq!(
        martin
            .get("/usda_naip_128_none_z2/18/42712/97343")
            .await
            .status(),
        200
    );

    watched.touch("usda_naip_128_none_z2.tif");
    martin
        .wait_for_log("Updated source source.id=usda_naip_128_none_z2")
        .await;

    watched.remove("usda_naip_128_none_z2.tif");
    martin
        .wait_for_source_removed("usda_naip_128_none_z2")
        .await;
    assert_eq!(
        martin
            .get("/usda_naip_128_none_z2/18/42712/97343")
            .await
            .status(),
        404
    );

    martin.stop().await;
    martin.assert_log_contains("Added source source.id=usda_naip_128_none_z2");
    martin.assert_log_contains("Updated source source.id=usda_naip_128_none_z2");
    martin.assert_log_contains("Removed source source.id=usda_naip_128_none_z2");
    martin.assert_log_contains(r#"ERROR error="Source usda_naip_128_none_z2 does not exist""#);
}

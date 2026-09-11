//! COG (`Cloud Optimized GeoTIFF`) sources: discovered in a directory, configured by id, and
//! reloaded while the server runs.
//!
//! Binary has to be built with `unstable-cog`.

#![cfg(feature = "test-cog")]

use std::fs;

use image::ImageFormat;
use martin_e2e_tests::{
    CogFixture, Martin, PROJECTED_CRS_GEO_KEY, StartError, StaticFiles, WatchedDir, fixture,
    round_floats, tag, temp_dir,
};
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
      paths: tests/fixtures/cog
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
async fn a_cog_url_is_read_over_http_using_ranges() {
    let tmp = tempfile::tempdir().expect("failed to create a temp dir");
    let save_config = tmp.path().join("save_config.yaml");
    let name = "usda_naip_512_webp_z5.tif";
    let statics = StaticFiles::serving(&[(name, fixture(&format!("cog/{name}")))]).await;
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

/// The COG README states the requirements a file has to meet. Each case breaks one of them in a
/// copy of a fixture that otherwise meets them all.
#[rstest]
#[case::compression_must_be_one_martin_can_decode(
    |cog: CogFixture| cog.set_short(0, tag::COMPRESSION, 32773),
    "The compression type 32773 of the tiff file"
)]
#[case::the_compression_must_be_stated(
    |cog: CogFixture| cog.remove_tag(0, tag::COMPRESSION),
    "Couldn't find tags [259]"
)]
#[case::the_planar_configuration_must_be_stated(
    |cog: CogFixture| cog.remove_tag(0, tag::PLANAR_CONFIGURATION),
    "Couldn't find tags [284]"
)]
#[case::the_planar_configuration_must_be_chunky(
    |cog: CogFixture| cog.set_short(0, tag::PLANAR_CONFIGURATION, 2),
    "as tiff file: format error: inconsistent sizes encountered"
)]
#[case::the_projected_crs_must_be_web_mercator(
    |cog: CogFixture| cog.set_geo_key(PROJECTED_CRS_GEO_KEY, 4326),
    "The projected coordinate reference system must be EPSG:3857"
)]
#[case::the_geo_keys_must_be_a_directory_martin_can_walk(
    |cog: CogFixture| cog.set_geo_key_header([1, 1, 0, 0]),
    "The projected coordinate reference system must be EPSG:3857"
)]
#[case::the_geo_keys_must_be_present(
    |cog: CogFixture| cog.remove_tag(0, tag::GEO_KEY_DIRECTORY),
    "The projected coordinate reference system must be EPSG:3857"
)]
#[case::the_pixels_must_be_square(
    |cog: CogFixture| cog.set_double(0, tag::MODEL_PIXEL_SCALE, 1, 99.0),
    "is not squared, the x_scale is 0.5971642834779395, the y_scale is 99"
)]
#[case::the_pixel_scale_must_have_three_values(
    |cog: CogFixture| cog.set_count(0, tag::MODEL_PIXEL_SCALE, 2),
    "The count of pixel scale should be 3"
)]
#[case::the_tie_points_must_come_in_sixes(
    |cog: CogFixture| cog.set_count(0, tag::MODEL_TIEPOINT, 5),
    "The count of tie points should be a multiple of 6"
)]
#[case::the_transformation_matrix_must_have_sixteen_values(
    |cog: CogFixture| cog.into_model_transformation().set_count(0, tag::MODEL_TRANSFORMATION, 12),
    "The length of matrix should be 16"
)]
#[case::the_image_must_be_georeferenced_at_all(
    |cog: CogFixture| cog.remove_tag(0, tag::MODEL_PIXEL_SCALE).remove_tag(0, tag::MODEL_TIEPOINT),
    "Either a valid transformation (tag 34264) or both pixel scale (tag 33550) and tie points (tag 33922) must be provided"
)]
#[case::every_overview_must_land_on_a_web_mercator_zoom(
    |cog: CogFixture| cog.set_short(0, tag::TILE_WIDTH, 300),
    "Calculating the image zoom level failed for"
)]
#[case::every_overview_must_use_the_same_tile_size(
    |cog: CogFixture| cog.set_short(2, tag::TILE_WIDTH, 512),
    "The size of each tile is not consistent."
)]
#[case::the_bands_must_be_a_color_type_martin_can_re_encode(
    |cog: CogFixture| cog.set_short(0, tag::PHOTOMETRIC_INTERPRETATION, 0),
    "The color type Multiband { bit_depth: 8, num_samples: 4 } and its bit depth"
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
    assert!(
        log.contains(expected),
        "log must say why the file was rejected, expected {expected:?}; log:\n{log}"
    );
}

#[tokio::test]
async fn a_tiff_stored_in_strips_rather_than_tiles_is_rejected() {
    let error = Martin::builder()
        .arg(fixture("files/striped_not_tiled.tif"))
        .start()
        .await
        .expect_err("martin must refuse to publish a striped TIFF");
    let StartError::EarlyExit { log, .. } = error else {
        panic!("expected an early exit, got: {error}");
    };
    assert!(
        log.contains("Striped tiff file is not supported"),
        "log must say the file is striped; log:\n{log}"
    );
}

/// An overview martin cannot read costs the zoom it would have served, rather than the source.
#[tokio::test]
async fn an_unreadable_overview_drops_only_that_zoom() {
    let tmp = temp_dir();
    let path = CogFixture::new("usda_naip_256_lzw_z3")
        .remove_tag(2, tag::IMAGE_WIDTH)
        .write_to(tmp.path(), "usda_naip_256_lzw_z3");
    let mut martin = Martin::builder()
        .arg(&path)
        .start()
        .await
        .expect("failed to start martin");

    let tilejson = tilejson(&martin, "usda_naip_256_lzw_z3").await;
    assert_eq!(tilejson["minzoom"], 17);
    assert_eq!(tilejson["maxzoom"], 18);
    assert_eq!(
        martin
            .get("/usda_naip_256_lzw_z3/17/21354/48672")
            .await
            .status(),
        200
    );

    martin.stop().await;
    martin.assert_log_clean();
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
    assert!(
        log.contains("as tiff file:"),
        "log must say the file could not be decoded; log:\n{log}"
    );
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
    let mut with_the_cog_dir = martin_with_the_cog_dir().await;

    assert_eq!(
        tilejson(&martin, "usda_naip_256_lzw_z3").await,
        tilejson(&with_the_cog_dir, "usda_naip_256_lzw_z3").await
    );
    let tile = martin.get("/usda_naip_256_lzw_z3/16/10677/24336").await;
    assert_eq!(tile.status(), 200);
    assert_eq!(tile.image_size(), (256, 256));

    with_the_cog_dir.stop().await;
    with_the_cog_dir.assert_log_clean();
    martin.stop().await;
    martin.assert_log_clean();
}

#[tokio::test]
async fn an_image_without_an_alpha_band_serves_a_tile_without_one() {
    let mut martin = martin_with_the_cog_dir().await;

    let tile = martin.get("/usda_naip_256_lzw_rgb_z2/18/42709/97344").await;
    assert_eq!(tile.status(), 200);
    assert_eq!(tile.header("content-type"), Some("image/png"));
    assert_eq!(tile.image_size(), (256, 256));
    assert_eq!(tile.image_color(), image::ColorType::Rgb8);

    martin.stop().await;
    martin.assert_log_clean();
}

/// A COG may leave a tile out of the file rather than store a blank one. This fixture stores only
/// tiles 10-11 of rows 14-16, so its very first tile is one of the gaps.
#[tokio::test]
async fn a_tile_the_image_leaves_out_is_empty() {
    let mut martin = martin_with_the_cog_dir().await;

    let stored = martin.get("/usda_naip_512_jpeg_z5/17/21354/48670").await;
    assert_eq!(stored.status(), 200);

    let left_out = martin.get("/usda_naip_512_jpeg_z5/17/21344/48656").await;
    assert_eq!(left_out.status(), 204);
    assert!(left_out.body().is_empty(), "a tile left out has no body");

    martin.stop().await;
    martin.assert_log_clean();
}

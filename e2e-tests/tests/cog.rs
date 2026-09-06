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

    let requests = statics.request_log().await;
    insta::assert_snapshot!(requests, @r"
    HEAD /cogtest/usda_naip_128_none_z2.tif no range
    GET /cogtest/usda_naip_128_none_z2.tif bytes=0-32767
    GET /cogtest/usda_naip_128_none_z2.tif bytes=1284-66819
    HEAD /cogtest/usda_naip_128_none_z2.tif no range
    GET /cogtest/usda_naip_128_none_z2.tif bytes=0-32767
    GET /cogtest/usda_naip_128_none_z2.tif bytes=1284-66819
    ");
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

    let requests = statics.request_log().await;
    insta::assert_snapshot!(requests, @r"
    HEAD /usda_naip_512_webp_z5.tif no range
    GET /usda_naip_512_webp_z5.tif bytes=0-28219
    GET /usda_naip_512_webp_z5.tif bytes=11166-11777
    ");
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

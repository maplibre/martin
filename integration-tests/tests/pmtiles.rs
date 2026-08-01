//! `PMTiles` sources: configured up front, discovered in a watched directory, or read from a
//! remote store over HTTP or S3.

use std::fs;

use martin_integration_tests::{Martin, StaticFiles, WatchedDir, fixture, vector_pmtiles};

/// The `tests/fixtures/pmtiles` directory, whose two files cover both a plain source id and one
/// that has to be derived from a file name carrying characters a URL cannot.
async fn martin_with_the_pmtiles_dir() -> Martin {
    Martin::builder()
        .arg("tests/fixtures/pmtiles")
        .start()
        .await
        .expect("failed to start martin")
}

/// Every start on that directory warns about the `+` in `stamen_toner__raster_CC-BY+ODbL_z3.pmtiles`,
/// which martin replaces with a `-` to keep the source id url-safe.
fn assert_the_file_name_was_sanitized_into_the_source_id(martin: &mut Martin) {
    martin.assert_log_contains(
        "Source was renamed: ID may only contain alpha-numeric characters or `._-` source.id.old=stamen_toner__raster_CC-BY+ODbL_z3 source.id.new=stamen_toner__raster_CC-BY-ODbL_z3",
    );
}

#[tokio::test]
async fn auto_configured_minimal() {
    let tmp = tempfile::tempdir().expect("failed to create a temp dir");
    let save_config = tmp.path().join("save_config.yaml");
    let mut martin = Martin::builder()
        .arg("--save-config")
        .arg(&save_config)
        .arg("tests/fixtures/pmtiles2")
        .start()
        .await
        .expect("failed to start martin");

    let catalog = martin.get("/catalog").await;
    assert_eq!(catalog.status(), 200);
    insta::assert_snapshot!(catalog.headers_snapshot(), @r"
    content-encoding: br
    content-type: application/json
    transfer-encoding: chunked
    vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    ");

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

    insta::with_settings!({sort_maps => true}, {
        insta::assert_json_snapshot!(catalog_json, @r#"
        {
          "fonts": {},
          "sprites": {},
          "styles": {},
          "tiles": {
            "webp2": {
              "content_type": "image/webp",
              "name": "ne2sr"
            }
          }
        }
        "#);
    });

    let tile = martin.get("/webp2/0/0/0").await;
    assert_eq!(tile.status(), 200);
    insta::assert_snapshot!(tile.headers_snapshot(), @r#"
    content-length: 11586
    content-type: image/webp
    etag: "wutUPc_mx5TO8aNmMnsK8A"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    assert_eq!(tile.image_size(), (512, 512));

    let saved = fs::read_to_string(&save_config).expect("martin did not write --save-config");
    insta::assert_snapshot!(saved, @r"
    listen_addresses: 127.0.0.1:0
    pmtiles:
      paths: tests/fixtures/pmtiles2
      sources:
        webp2: tests/fixtures/pmtiles2/webp2.pmtiles
    mbtiles: tests/fixtures/pmtiles2
    geojson: tests/fixtures/pmtiles2
    ");

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_directory_publishes_a_source_per_file_under_a_url_safe_id() {
    let mut martin = martin_with_the_pmtiles_dir().await;

    let catalog = martin.get("/catalog").await;
    insta::with_settings!({sort_maps => true}, {
        insta::assert_json_snapshot!(catalog.json()["tiles"], @r#"
        {
          "png": {
            "content_type": "image/png",
            "name": "ne2sr"
          },
          "stamen_toner__raster_CC-BY-ODbL_z3": {
            "content_type": "image/png"
          }
        }
        "#);
    });

    martin.stop().await;
    assert_the_file_name_was_sanitized_into_the_source_id(&mut martin);
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_raster_source_serves_its_tilejson() {
    let mut martin = martin_with_the_pmtiles_dir().await;

    let response = martin.get("/stamen_toner__raster_CC-BY-ODbL_z3").await;
    assert_eq!(response.status(), 200);
    insta::with_settings!({filters => vec![(r"(?m)^etag: .*$", "etag: [ETAG]")]}, {
        insta::assert_snapshot!(response.headers_snapshot(), @r"
        content-encoding: br
        content-type: application/json
        etag: [ETAG]
        transfer-encoding: chunked
        vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
        ");
    });
    let tilejson = serde_json::from_str::<serde_json::Value>(&martin.redact(&response.text()))
        .expect("response body is not valid json");
    insta::assert_json_snapshot!(tilejson, @r#"
    {
      "bounds": [
        -180.0,
        -85.0,
        180.0,
        85.0
      ],
      "center": [
        0.0,
        0.0,
        0
      ],
      "maxzoom": 3,
      "minzoom": 0,
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/stamen_toner__raster_CC-BY-ODbL_z3/{z}/{x}/{y}"
      ]
    }
    "#);

    martin.stop().await;
    assert_the_file_name_was_sanitized_into_the_source_id(&mut martin);
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_raster_source_serves_png_tiles() {
    let mut martin = martin_with_the_pmtiles_dir().await;

    let tile = martin
        .get("/stamen_toner__raster_CC-BY-ODbL_z3/3/4/2")
        .await;
    assert_eq!(tile.status(), 200);
    insta::assert_snapshot!(tile.headers_snapshot(), @r#"
    content-length: 24475
    content-type: image/png
    etag: "I1fhCKy04n2xAQpEk2ESig"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    assert_eq!(tile.image_size(), (256, 256));

    martin.stop().await;
    assert_the_file_name_was_sanitized_into_the_source_id(&mut martin);
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_configured_source_serves_tiles_under_the_id_it_was_given() {
    let mut martin = Martin::builder()
        .config(
            "
pmtiles:
  sources:
    pmt:
      path: tests/fixtures/pmtiles/stamen_toner__raster_CC-BY+ODbL_z3.pmtiles
      cache:
        minzoom: 0
        maxzoom: 3
",
        )
        .start()
        .await
        .expect("failed to start martin");

    insta::assert_json_snapshot!(martin.get("/catalog").await.json()["tiles"], @r#"
    {
      "pmt": {
        "content_type": "image/png"
      }
    }
    "#);

    let tile = martin.get("/pmt/0/0/0").await;
    assert_eq!(tile.status(), 200);
    insta::assert_snapshot!(tile.headers_snapshot(), @r#"
    content-length: 18404
    content-type: image/png
    etag: "aKKkpu0hTRlf8joPaDt3Ug"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    assert_eq!(tile.image_size(), (256, 256));

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

/// A server holding the one fixture the remote tests read, under `name`.
async fn statics_serving(name: &str) -> StaticFiles {
    StaticFiles::serving(&[(name, fixture("pmtiles2/webp2.pmtiles"))]).await
}

#[tokio::test]
async fn a_source_url_is_read_over_http() {
    let tmp = tempfile::tempdir().expect("failed to create a temp dir");
    let save_config = tmp.path().join("save_config.yaml");
    let statics = statics_serving("webp2.pmtiles").await;
    let mut martin = Martin::builder()
        .arg("--save-config")
        .arg(&save_config)
        .arg(statics.url("webp2.pmtiles"))
        .start()
        .await
        .expect("failed to start martin");

    insta::assert_json_snapshot!(martin.get("/catalog").await.json()["tiles"], @r#"
    {
      "webp2": {
        "content_type": "image/webp",
        "name": "ne2sr"
      }
    }
    "#);

    let saved = fs::read_to_string(&save_config).expect("martin did not write --save-config");
    insta::with_settings!({filters => vec![(r"http://127\.0\.0\.1:\d+", "http://[STATICS]")]}, {
        insta::assert_snapshot!(saved, @r"
        listen_addresses: 127.0.0.1:0
        pmtiles:
          sources:
            webp2: http://[STATICS]/webp2.pmtiles
        ");
    });

    let tile = martin.get("/webp2/1/0/0").await;
    assert_eq!(tile.status(), 200);
    insta::assert_snapshot!(tile.headers_snapshot(), @r#"
    content-length: 10658
    content-type: image/webp
    etag: "YQCG8_HEN_sExq050B7MCQ"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    assert_eq!(tile.image_size(), (512, 512));

    martin.stop().await;
    insta::assert_snapshot!(statics.request_log().await, @"
    GET /webp2.pmtiles bytes=0-16383
    GET /webp2.pmtiles bytes=171-314
    GET /webp2.pmtiles bytes=11901-22558
    ");
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_configured_source_url_is_read_over_http() {
    let statics = statics_serving("webp2.pmtiles").await;
    let mut martin = Martin::builder()
        .config(&format!(
            "
pmtiles:
  sources:
    pmt2: {}
",
            statics.url("webp2.pmtiles")
        ))
        .start()
        .await
        .expect("failed to start martin");

    let tile = martin.get("/pmt2/0/0/0").await;
    assert_eq!(tile.status(), 200);
    insta::assert_snapshot!(tile.headers_snapshot(), @r#"
    content-length: 11586
    content-type: image/webp
    etag: "wutUPc_mx5TO8aNmMnsK8A"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    assert_eq!(tile.image_size(), (512, 512));

    martin.stop().await;
    insta::assert_snapshot!(statics.request_log().await, @"
    GET /webp2.pmtiles bytes=0-16383
    GET /webp2.pmtiles bytes=171-314
    GET /webp2.pmtiles bytes=315-11900
    ");
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

/// A config reading `s3://pmtilestest/{file}` from a [`StaticFiles`] server rather than from AWS:
/// path-style addressing turns the bucket into the first path segment, and unsigned requests keep
/// credentials out of it.
fn s3_config(statics: &StaticFiles, id: &str, file: &str) -> String {
    format!(
        "
pmtiles:
  aws_endpoint: {}
  aws_region: eu-central-1
  skip_signature: true
  allow_http: true
  virtual_hosted_style_request: false
  sources:
    {id}: s3://pmtilestest/{file}
",
        statics.base_url()
    )
}

/// The two `AWS_*` variables [`Martin::builder`] sets, which an `s3_config` overrides.
fn assert_the_aws_environment_was_overridden(martin: &mut Martin) {
    martin.assert_log_contains(
        "Environment variable AWS_SKIP_CREDENTIALS is ignored in favor of the new configuration value pmtiles.skip_signature.",
    );
    martin.assert_log_contains(
        "Environment variable AWS_REGION is ignored in favor of the new configuration value pmtiles.aws_region.",
    );
}

#[tokio::test]
async fn a_configured_source_is_read_from_an_s3_bucket() {
    let statics = statics_serving("pmtilestest/webp2.pmtiles").await;
    let mut martin = Martin::builder()
        .config(&s3_config(&statics, "s3", "webp2.pmtiles"))
        .start()
        .await
        .expect("failed to start martin");

    insta::assert_json_snapshot!(martin.get("/catalog").await.json()["tiles"], @r#"
    {
      "s3": {
        "content_type": "image/webp",
        "name": "ne2sr"
      }
    }
    "#);

    let tile = martin.get("/s3/1/0/0").await;
    assert_eq!(tile.status(), 200);
    insta::assert_snapshot!(tile.headers_snapshot(), @r#"
    content-length: 10658
    content-type: image/webp
    etag: "YQCG8_HEN_sExq050B7MCQ"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    assert_eq!(tile.image_size(), (512, 512));

    martin.stop().await;
    insta::assert_snapshot!(statics.request_log().await, @"
    GET /pmtilestest/webp2.pmtiles bytes=0-16383
    GET /pmtilestest/webp2.pmtiles bytes=171-314
    GET /pmtilestest/webp2.pmtiles bytes=11901-22558
    ");
    assert_the_aws_environment_was_overridden(&mut martin);
    martin.assert_log_clean();
}

/// The remote-store tests above read a raster archive, whose tiles travel uncompressed. A vector
/// archive stores its tiles gzipped, and martin hands those to the client still gzipped rather
/// than recompressing them.
#[tokio::test]
async fn a_vector_source_is_served_gzipped_from_a_remote_store() {
    let tmp = tempfile::tempdir().expect("failed to create a temp dir");
    let archive = vector_pmtiles(
        fixture("mbtiles/world_cities.mbtiles"),
        tmp.path().join("world_cities.pmtiles"),
    )
    .await;
    let statics = StaticFiles::serving(&[("pmtilestest/world_cities.pmtiles", archive)]).await;
    let mut martin = Martin::builder()
        .config(&s3_config(&statics, "cities", "world_cities.pmtiles"))
        .start()
        .await
        .expect("failed to start martin");

    insta::assert_json_snapshot!(martin.get("/catalog").await.json()["tiles"], @r#"
    {
      "cities": {
        "content_encoding": "gzip",
        "content_type": "application/x-protobuf",
        "description": "Major cities from Natural Earth data",
        "name": "Major cities from Natural Earth data"
      }
    }
    "#);

    let tile = martin.get("/cities/2/3/2").await;
    assert_eq!(tile.status(), 200);
    insta::assert_snapshot!(tile.headers_snapshot(), @r#"
    content-encoding: gzip
    content-length: 151
    content-type: application/x-protobuf
    etag: "6ytE8xWaxj7fMiMIkSSO6Q"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    insta::assert_snapshot!(tile.mvt_dump(), @r#"
    layer: 0
      name: cities
      version: 2
      extent: 4096
      feature: 0
        id: (none)
        geometry: POINT(630,-59)
        properties:
          name = "Singapore"
      feature: 1
        id: (none)
        geometry: POINT(765,281)
        properties:
          name = "Jakarta"
      feature: 2
        id: (none)
        geometry: POINT(2501,1861)
        properties:
          name = "Melbourne"
      feature: 3
        id: (none)
        geometry: POINT(2784,1642)
        properties:
          name = "Sydney"
      feature: 4
        id: (none)
        geometry: POINT(3857,1806)
        properties:
          name = "Auckland"
    "#);

    martin.stop().await;
    insta::assert_snapshot!(statics.request_log().await, @"
    GET /pmtilestest/world_cities.pmtiles bytes=0-16383
    GET /pmtilestest/world_cities.pmtiles bytes=16384-17147
    GET /pmtilestest/world_cities.pmtiles bytes=18275-18425
    ");
    assert_the_aws_environment_was_overridden(&mut martin);
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_source_is_read_from_a_bucket_on_the_real_aws() {
    let tmp = tempfile::tempdir().expect("failed to create a temp dir");
    let save_config = tmp.path().join("save_config.yaml");
    let mut martin = Martin::builder()
        .arg("--save-config")
        .arg(&save_config)
        .arg("s3://pmtilestest/cb_2018_us_zcta510_500k.pmtiles")
        .start()
        .await
        .expect("failed to start martin");

    insta::assert_json_snapshot!(martin.get("/catalog").await.json()["tiles"], @r#"
    {
      "cb_2018_us_zcta510_500k": {
        "content_encoding": "gzip",
        "content_type": "application/x-protobuf",
        "description": "cb_2018_us_zcta510_500k.mbtiles",
        "name": "cb_2018_us_zcta510_500k.mbtiles"
      }
    }
    "#);

    let tile = martin.get("/cb_2018_us_zcta510_500k/1/0/0").await;
    assert_eq!(tile.status(), 200);
    insta::assert_snapshot!(tile.headers_snapshot(), @r#"
    content-encoding: gzip
    content-length: 215320
    content-type: application/x-protobuf
    etag: "5_Cffo9I87z38gagDXRhRA"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    let layers = tile.mvt().layers;
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].name, "zcta");
    assert_eq!(layers[0].features.len(), 6523);

    let saved = fs::read_to_string(&save_config).expect("martin did not write --save-config");
    insta::assert_snapshot!(saved, @r"
    listen_addresses: 127.0.0.1:0
    pmtiles:
      sources:
        cb_2018_us_zcta510_500k: s3://pmtilestest/cb_2018_us_zcta510_500k.pmtiles
    ");

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn route_prefix_keeps_root_health() {
    let mut martin = Martin::builder()
        .arg("--route-prefix")
        .arg("/foo")
        .arg("tests/fixtures/pmtiles2")
        .start()
        .await
        .expect("failed to start martin");

    let root = martin.get("/health").await;
    assert_eq!(root.status(), 200);
    assert_eq!(root.text(), "OK");

    let prefixed = martin.get("/foo/health").await;
    assert_eq!(prefixed.status(), 200);
    assert_eq!(prefixed.text(), "OK");

    martin.stop().await;
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn reload_adds_updates_and_removes_a_source() {
    let watched = WatchedDir::new();
    let mut martin = Martin::builder()
        .arg(watched.dir())
        .start()
        .await
        .expect("failed to start martin");

    let catalog = martin.get("/catalog").await;
    assert_eq!(catalog.json()["tiles"], serde_json::json!({}));
    insta::assert_snapshot!(catalog.headers_snapshot(), @r"
    content-encoding: br
    content-type: application/json
    transfer-encoding: chunked
    vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    ");
    assert_eq!(martin.get("/png/0/0/0").await.status(), 404);

    watched.install(fixture("pmtiles/png.pmtiles"), "png.pmtiles");
    martin.wait_for_source("png").await;
    insta::assert_json_snapshot!(martin.get("/catalog").await.json()["tiles"], @r#"
    {
      "png": {
        "content_type": "image/png",
        "name": "ne2sr"
      }
    }
    "#);

    let tile = martin.get("/png/0/0/0").await;
    assert_eq!(tile.status(), 200);
    assert_eq!(tile.image_size(), (512, 512));

    watched.touch("png.pmtiles");
    martin.wait_for_log("Updated source source.id=png").await;

    watched.remove("png.pmtiles");
    martin.wait_for_source_removed("png").await;
    assert_eq!(martin.get("/png/0/0/0").await.status(), 404);

    martin.stop().await;
    martin.assert_log_contains("Added source source.id=png");
    martin.assert_log_contains("Updated source source.id=png");
    martin.assert_log_contains("Removed source source.id=png");
    martin.assert_log_contains(r#"ERROR error="Source png does not exist""#);
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn reload_removes_a_source_present_at_startup() {
    let watched = WatchedDir::new();
    watched.seed(fixture("pmtiles/png.pmtiles"), "png.pmtiles");

    let mut martin = Martin::builder()
        .arg(watched.dir())
        .start()
        .await
        .expect("failed to start martin");

    martin.wait_for_source("png").await;
    assert_eq!(martin.get("/png/0/0/0").await.status(), 200);

    watched.remove("png.pmtiles");
    martin.wait_for_source_removed("png").await;

    martin.stop().await;
    martin.assert_log_contains("Removed source source.id=png");
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

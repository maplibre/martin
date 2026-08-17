use std::fs;

use martin_e2e_tests::{Martin, MartinBuilder};
use tempfile::TempDir;

const CONFIG: &str = "
keep_alive: 75
cache:
  size_mb: 8
  minzoom: 0
  maxzoom: 20
observability:
  metrics:
    add_labels: {}
cors:
  origin:
    - '*'
  max_age: null
tilejson_url_version_param: version
pmtiles:
  sources:
    pmt:
      path: tests/fixtures/pmtiles/stamen_toner__raster_CC-BY+ODbL_z3.pmtiles
      cache:
        minzoom: 0
        maxzoom: 3
geojson: {}
sprites:
  paths: tests/fixtures/sprites/src1
  sources:
    mysrc: tests/fixtures/sprites/src2
styles:
  sources:
    maplibre: tests/fixtures/styles/maplibre_demo.json
  paths:
    - tests/fixtures/styles/src2
fonts:
  - tests/fixtures/fonts/overpass-mono-regular.ttf
  - tests/fixtures/fonts
";

/// Start martin with `--save-config` and hand back what it wrote.
async fn saved_config(builder: MartinBuilder) -> (Martin, String) {
    let dir = TempDir::new().expect("failed to create a temp dir");
    let save_config = dir.path().join("save_config.yaml");
    let martin = builder
        .arg("--save-config")
        .arg(&save_config)
        .start()
        .await
        .expect("failed to start martin");
    let saved = fs::read_to_string(&save_config).expect("martin did not write --save-config");
    (martin, saved)
}

#[tokio::test]
async fn every_discovered_source_and_resource_is_spelled_out() {
    let (mut martin, saved) = saved_config(
        Martin::builder()
            .arg("tests/fixtures/pmtiles")
            .arg("--sprite")
            .arg("tests/fixtures/sprites/src1")
            .arg("--font")
            .arg("tests/fixtures/fonts/overpass-mono-regular.ttf")
            .arg("--font")
            .arg("tests/fixtures/fonts")
            .arg("--style")
            .arg("tests/fixtures/styles/maplibre_demo.json")
            .arg("--style")
            .arg("tests/fixtures/styles/src2")
            .arg("--tilejson-url-version-param")
            .arg("version"),
    )
    .await;

    insta::assert_snapshot!(saved);

    martin.stop().await;
    martin.assert_log_contains(
        "Source was renamed: ID may only contain alpha-numeric characters or `._-` source.id.old=stamen_toner__raster_CC-BY+ODbL_z3 source.id.new=stamen_toner__raster_CC-BY-ODbL_z3",
    );
    martin.assert_log_contains(
        "Ignoring duplicate font: already configured from another path font.name=Overpass Mono Regular",
    );
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn every_documented_setting_survives_the_round_trip() {
    let (mut martin, saved) =
        saved_config(Martin::builder().arg("--workers").arg("1").config(CONFIG)).await;

    insta::assert_snapshot!(saved);

    martin.stop().await;
    martin.assert_log_contains(
        "Ignoring duplicate font: already configured from another path font.name=Overpass Mono Regular",
    );
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

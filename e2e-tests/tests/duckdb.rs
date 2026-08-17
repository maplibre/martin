//! `GeoParquet` sources served through `DuckDB`, configured from a config file in a temp dir.
//!
//! Binary has to be buld with `unstable-duckdb`.

#![cfg(feature = "test-duckdb")]

use std::fs;

use martin_e2e_tests::{Martin, MartinBuilder};
use rstest::rstest;
use tempfile::TempDir;

const POLYGONS: &str = "\
duckdb:
  sources:
    - geoparquet: tests/fixtures/duckdb/geoparquet_polygons.parquet
      layer_id: polygons
      geometry_column: geom
      srid: 4326
      minzoom: 0
      maxzoom: 14
";

const POLYGONS_WITH_A_SIBLING_NAMING_AN_ABSENT_GEOMETRY_COLUMN: &str = "\
on_invalid: warn
duckdb:
  sources:
    - geoparquet: tests/fixtures/duckdb/geoparquet_polygons.parquet
      layer_id: polygons
      geometry_column: geom
      srid: 4326
    - geoparquet: tests/fixtures/duckdb/geoparquet_polygons.parquet
      layer_id: polygons_bad
      geometry_column: nonexistent
      srid: 4326
";

const POLYGONS_WITH_A_SIBLING_NAMING_AN_ABSENT_ID_COLUMN: &str = "\
on_invalid: warn
duckdb:
  sources:
    - geoparquet: tests/fixtures/duckdb/geoparquet_polygons.parquet
      layer_id: polygons
      geometry_column: geom
      srid: 4326
    - geoparquet: tests/fixtures/duckdb/geoparquet_polygons.parquet
      layer_id: polygons_bad
      geometry_column: geom
      id_column: nonexistent
      srid: 4326
";

fn temp_dir() -> TempDir {
    tempfile::tempdir().expect("failed to create a temp dir")
}

fn martin_with_config(dir: &TempDir, yaml: &str) -> MartinBuilder {
    let config = dir.path().join("config.yaml");
    fs::write(&config, yaml).expect("failed to write the config file");
    Martin::builder().arg("--config").arg(config)
}

async fn start_with_config(dir: &TempDir, yaml: &str) -> Martin {
    martin_with_config(dir, yaml)
        .start()
        .await
        .expect("failed to start martin")
}

#[tokio::test]
async fn the_catalog_names_the_geoparquet_file() {
    let dir = temp_dir();
    let mut martin = start_with_config(&dir, POLYGONS).await;

    let catalog = martin.get("/catalog").await;
    assert_eq!(catalog.status(), 200);
    insta::assert_snapshot!(catalog.headers_snapshot(), @r"
    content-encoding: br
    content-type: application/json
    transfer-encoding: chunked
    vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    ");
    insta::assert_json_snapshot!(catalog.json()["tiles"], @r#"
    {
      "polygons": {
        "content_type": "application/x-protobuf",
        "description": "GeoParquet (tests/fixtures/duckdb/geoparquet_polygons.parquet)"
      }
    }
    "#);

    martin.stop().await;
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_tilejson_describes_the_parquet_columns() {
    let dir = temp_dir();
    let mut martin = start_with_config(&dir, POLYGONS).await;

    let response = martin.get("/polygons").await;
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
        -50.0,
        20.0,
        5.0,
        30.0
      ],
      "description": "GeoParquet (tests/fixtures/duckdb/geoparquet_polygons.parquet)",
      "maxzoom": 14,
      "minzoom": 0,
      "name": "polygons",
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/polygons/{z}/{x}/{y}"
      ],
      "vector_layers": [
        {
          "fields": {
            "id": "INTEGER",
            "name": "VARCHAR"
          },
          "id": "polygons"
        }
      ]
    }
    "#);

    martin.stop().await;
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_tile_clips_the_polygon_crossing_its_edge_and_keeps_the_one_inside_it() {
    let dir = temp_dir();
    let mut martin = start_with_config(&dir, POLYGONS).await;

    let tile = martin.get("/polygons/1/0/0").await;
    assert_eq!(tile.status(), 200);
    insta::assert_snapshot!(tile.headers_snapshot(), @r#"
    content-encoding: gzip
    content-length: 146
    content-type: application/x-protobuf
    etag: "onJtfkQNRX7OcBJtdeL9MQ"
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    "#);
    insta::assert_snapshot!(tile.mvt_dump(), @r#"
    layer: 0
      name: polygons
      version: 2
      extent: 4096
      feature: 0
        id: (none)
        geometry: RING[count=5](3982 3380,4160 3380,4160 3631,3982 3631,3982 3380)[OUTER]
        properties:
          id = 1 (int)
          name = "boundary_span"
      feature: 1
        id: (none)
        geometry: RING[count=5](3186 3631,2958 3631,2958 3380,3186 3380,3186 3631)[OUTER]
        properties:
          id = 2 (int)
          name = "inside_west"
    "#);

    martin.stop().await;
    martin.assert_log_clean();
}

#[tokio::test]
async fn the_saved_config_fills_in_the_source_defaults() {
    let dir = temp_dir();
    let save_config = dir.path().join("save_config.yaml");
    let mut martin = martin_with_config(&dir, POLYGONS)
        .arg("--save-config")
        .arg(&save_config)
        .start()
        .await
        .expect("failed to start martin");

    let saved = fs::read_to_string(&save_config).expect("martin did not write --save-config");
    insta::assert_snapshot!(saved, @r"
    listen_addresses: 127.0.0.1:0
    duckdb:
      sources:
      - geoparquet: tests/fixtures/duckdb/geoparquet_polygons.parquet
        layer_id: polygons
        geometry_column: geom
        srid: 4326
        minzoom: 0
        maxzoom: 14
        pool_size: 4
        auto_bounds: quick
    ");

    martin.stop().await;
    martin.assert_log_clean();
}

#[rstest]
#[case::an_absent_geometry_column(
    POLYGONS_WITH_A_SIBLING_NAMING_AN_ABSENT_GEOMETRY_COLUMN,
    "GeoParquet geometry column 'nonexistent' was not found"
)]
#[case::an_absent_id_column(
    POLYGONS_WITH_A_SIBLING_NAMING_AN_ABSENT_ID_COLUMN,
    "GeoParquet id_column 'nonexistent' was not found"
)]
#[tokio::test]
async fn an_invalid_sibling_warns_without_taking_down_the_valid_source(
    #[case] config: &str,
    #[case] warning: &str,
) {
    let dir = temp_dir();
    let mut martin = start_with_config(&dir, config).await;

    let tiles = martin.get("/catalog").await.json()["tiles"].clone();
    assert!(tiles.get("polygons").is_some(), "catalog: {tiles}");
    assert!(tiles.get("polygons_bad").is_none(), "catalog: {tiles}");
    assert_eq!(martin.get("/polygons/1/0/0").await.status(), 200);
    assert_eq!(martin.get("/polygons_bad/1/0/0").await.status(), 404);

    martin.stop().await;
    martin.assert_log_contains(&format!(
        "Tile source resolution warning: Source polygons_bad: {warning}"
    ));
    martin.assert_log_contains(r#"ERROR error="Source polygons_bad does not exist""#);
    martin.assert_log_clean();
}

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

const MIXED_TYPES: &str = "\
duckdb:
  sources:
    - geoparquet: tests/fixtures/duckdb/geoparquet_mixed_types.parquet
      layer_id: mixed
      geometry_column: geom
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
    insta::assert_snapshot!(catalog.headers_snapshot_masking_etag(), @"
    content-encoding: br
    content-type: application/json
    etag: [ETAG]
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
}

#[tokio::test]
async fn a_tilejson_describes_the_parquet_columns() {
    let dir = temp_dir();
    let mut martin = start_with_config(&dir, POLYGONS).await;

    let response = martin.get("/polygons").await;
    assert_eq!(response.status(), 200);
    insta::with_settings!({filters => vec![(r"(?m)^etag: .*$", "etag: [ETAG]")]}, {
        insta::assert_snapshot!(response.headers_snapshot(), @"
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
    insta::assert_snapshot!(saved, @"
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
}

#[tokio::test]
async fn a_tile_casts_the_property_columns_mvt_cannot_carry_and_drops_the_rest() {
    let dir = temp_dir();
    let mut martin = martin_with_config(&dir, MIXED_TYPES)
        .env("TZ", "UTC")
        .start()
        .await
        .expect("failed to start martin");

    let tilejson = martin.get("/mixed").await.json();
    insta::assert_json_snapshot!(tilejson["vector_layers"], @r#"
    [
      {
        "fields": {
          "area": "DOUBLE",
          "capacity": "INTEGER",
          "catalog_ref": "VARCHAR",
          "closes_at": "VARCHAR",
          "details": "VARCHAR",
          "endowment": "BIGINT",
          "external_id": "VARCHAR",
          "floor": "INTEGER",
          "id": "INTEGER",
          "ingested_at": "VARCHAR",
          "is_open": "BOOLEAN",
          "name": "VARCHAR",
          "opened": "VARCHAR",
          "opens_at": "VARCHAR",
          "population": "BIGINT",
          "published_at": "VARCHAR",
          "rating": "FLOAT",
          "ratio": "DOUBLE",
          "stars": "INTEGER",
          "surveyed_at": "VARCHAR",
          "tour_length": "VARCHAR",
          "visitors": "INTEGER"
        },
        "id": "mixed"
      }
    ]
    "#);

    let tile = martin.get("/mixed/1/0/0").await;
    assert_eq!(tile.status(), 200);
    insta::assert_snapshot!(tile.mvt_dump(), @r#"
    layer: 0
      name: mixed
      version: 2
      extent: 4096
      feature: 0
        id: (none)
        geometry: RING[count=5](3982 3380,4160 3380,4160 3631,3982 3631,3982 3380)[OUTER]
        properties:
          area = 1234.5678 (double)
          capacity = 65535 (int)
          catalog_ref = "18446744073709551615"
          closes_at = "15:45:15+00"
          details = "{\"kind\":\"park\"}"
          endowment = 9223372036854775807 (int)
          external_id = "5b8d1a4e-1e6c-4c6f-9b1a-2f0d3c4b5a69"
          floor = -128 (int)
          id = 1 (int)
          ingested_at = "2020-01-02 03:04:05.123456789"
          is_open = true (bool)
          name = "boundary_span"
          opened = "2020-01-02"
          opens_at = "08:30:00"
          population = 4294967295 (int)
          published_at = "2020-01-02 03:04:05+00"
          rating = 4.5 (float)
          ratio = 0.5 (double)
          stars = 0 (int)
          surveyed_at = "2020-01-02 03:04:05"
          tour_length = "01:30:00"
          visitors = 120 (int)
      feature: 1
        id: (none)
        geometry: RING[count=5](3186 3631,2958 3631,2958 3380,3186 3380,3186 3631)[OUTER]
        properties:
          area = -0.125 (double)
          capacity = 1 (int)
          catalog_ref = "0"
          closes_at = "13:30:00+00"
          details = "{\"kind\":\"museum\"}"
          endowment = -9223372036854775808 (int)
          external_id = "9f14c3d2-7b0a-4d5e-8c11-6a2b3e4f5061"
          floor = 127 (int)
          id = 2 (int)
          ingested_at = "2021-06-30 23:59:59.987654321"
          is_open = false (bool)
          name = "inside_west"
          opened = "2021-06-30"
          opens_at = "17:45:15"
          population = 7 (int)
          published_at = "2021-06-30 23:59:59+00"
          rating = -2.25 (float)
          ratio = 12.25 (double)
          stars = 255 (int)
          surveyed_at = "2021-06-30 23:59:59"
          tour_length = "3 days"
          visitors = -3 (int)
    "#);

    martin.stop().await;
    martin.assert_log_contains(
        "Ignoring 5 columns of tests/fixtures/duckdb/geoparquet_mixed_types.parquet with no MVT representation: address (STRUCT(street VARCHAR, city VARCHAR)), attributes (MAP(VARCHAR, VARCHAR)), centroid (GEOMETRY('OGC:CRS84')), tags (VARCHAR[]), thumbnail (BLOB). Vector tiles can only carry text, numeric and boolean properties.",
    );
}

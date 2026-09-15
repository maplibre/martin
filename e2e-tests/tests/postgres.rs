//! `PostgreSQL` sources: the ones martin discovers by itself, and the ones a config file declares.

#![cfg(feature = "test-pg")]

use std::fs;

use martin_e2e_tests::{
    Martin, MartinBuilder, StartError, mbtiles_fixture, round_floats, temp_dir,
};
use serde_json::Value;
use tempfile::TempDir;

/// A server that publishes everything it finds in the fixture database.
async fn martin_with_postgres() -> Martin {
    start_with_postgres(Martin::builder()).await
}

async fn start_with_postgres(builder: MartinBuilder) -> Martin {
    builder
        .with_postgres()
        // also adopt tables whose geometry column has SRID 0
        .arg("--default-srid")
        .arg("900913")
        // no fuzzy estimated bounds
        .arg("--auto-bounds")
        .arg("calc")
        // to not exhaust the global pool
        .arg("--pool-size")
        .arg("1")
        .start()
        .await
        .expect("failed to start martin")
}

/// The `PostgreSQL` config the [`CONFIG`] tests below start from: a few hand-written table and
/// function sources, plus an `auto_publish` narrowed to the schemas they do not cover.
const CONFIG: &str = "
postgres:
  connection_string: ${DATABASE_URL}
  default_srid: 4326
  pool_size: 1
  auto_publish:
    tables:
      from_schemas: autodetect
      id_columns: [feat_id, big_feat_id]
      clip_geom: false
      buffer: 3
      extent: 9000
    functions:
      from_schemas: MixedCase
  tables:
    table_source:
      schema: public
      table: table_source
      srid: 4326
      geometry_column: geom
      id_column: ~
      minzoom: 0
      maxzoom: 30
      bounds: [-180.0, -90.0, 180.0, 90.0]
      extent: 4096
      buffer: 64
      clip_geom: true
      geometry_type: GEOMETRY
      properties:
        gid: int4
    MixPoints:
      schema: MIXEDCASE
      table: MixPoints
      id_column: giD
      geometry_column: geoM
      srid: 4326
      geometry_type: POINT
      properties:
        taBLe: text
    points1:
      layer_id: abc
      schema: public
      table: points1
      srid: 4326
      geometry_column: geom
      minzoom: 0
      maxzoom: 30
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    points2:
      schema: public
      table: points2
      srid: 4326
      geometry_column: geom
      minzoom: 0
      maxzoom: 30
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    points1_filtered:
      layer_id: filtered
      schema: public
      table: points1
      srid: 4326
      geometry_column: geom
      geometry_type: POINT
      filter: gid <= 3
      properties:
        gid: int4
    points3857:
      schema: public
      table: points3857
      srid: 3857
      geometry_column: geom
      minzoom: 0
      maxzoom: 30
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
  functions:
    function_zxy_query:
      schema: public
      function: function_zxy_query
      minzoom: 0
      maxzoom: 30
      bounds: [-180.0, -90.0, 180.0, 90.0]
    function_zxy_query_test:
      schema: public
      function: function_zxy_query_test
      minzoom: 0
      maxzoom: 30
      bounds: [-180.0, -90.0, 180.0, 90.0]
";

/// A server that publishes what [`CONFIG`] declares.
async fn martin_from_the_config() -> Martin {
    start_from_the_config(Martin::builder()).await
}

async fn start_from_the_config(builder: MartinBuilder) -> Martin {
    builder
        .with_postgres()
        .config(CONFIG)
        .start()
        .await
        .expect("failed to start martin")
}

/// The tables martin walks past while resolving `auto_publish` warn about their missing index even
/// when they are not published.
fn assert_unindexed_table_warnings(martin: &mut Martin) {
    for warning in [
        "Table public.mat_view has no spatial index on column geom",
        "Table public.table_source has no spatial index on column geom",
        "Table public.table_source_geog has no spatial index on column geog",
    ] {
        martin.assert_log_contains(warning);
    }
}

fn assert_discovery_warnings(martin: &mut Martin) {
    assert_unindexed_table_warnings(martin);
    for warning in [
        "Not computing the bounds of public.mars_points.geom: SRID 949900 is IAU_2015:49900, not an EPSG system",
        "source.id.new=table_source_multiple_geom.1",
        "source.id.new=table_name_existing_two_schemas.1",
        "source.id.new=view_name_existing_two_schemas.1",
        "source.id.new=table_and_view_two_schemas.1",
        "source.id.new=function_dup.1",
        "source.id.new=function_pair_query.1",
        "source.id.new=function_two_schemas.1",
        "source.id.new=-function.withweired---_-characters",
        "source.id.new=.-Points-----------quote",
    ] {
        martin.assert_log_contains(warning);
    }
}

/// The `TileJSON` of `path`, with this instance's address replaced by a stable placeholder and
/// every float rounded to ten digits, which is as far as the bounds agree across platforms.
async fn tilejson(martin: &Martin, path: &str) -> Value {
    let response = martin.get(path).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.header("content-type"), Some("application/json"));
    let mut tilejson = serde_json::from_str::<Value>(&martin.redact(&response.text()))
        .expect("response body is not valid json");
    round_floats(&mut tilejson);
    tilejson
}

/// A vector tile from `path`, in the text form `mvt dump` prints.
async fn tile_dump(martin: &Martin, path: &str) -> String {
    let response = martin.get(path).await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.header("content-type"),
        Some("application/x-protobuf")
    );
    response.mvt_dump()
}

/// Ask `source` for every zoom level in [`ZOOMS`] and snapshot each tile.
async fn assert_tiles_across_zooms(martin: &Martin, source: &str, snapshot_prefix: &str) {
    for zxy in [
        "0/0/0",
        "6/57/29",
        "12/3673/1911",
        "13/7346/3822",
        "14/14692/7645",
        "17/117542/61161",
        "18/235085/122323",
    ] {
        let dump = tile_dump(martin, &format!("/{source}/{zxy}")).await;
        insta::assert_snapshot!(format!("{snapshot_prefix}_{}", zxy.replace('/', "_")), dump);
    }
}

#[tokio::test]
async fn legacy_postgres_env_vars_warn_in_the_log() {
    let mut martin = Martin::builder()
        .with_postgres()
        .env("DEFAULT_SRID", "4326")
        .start()
        .await
        .expect("failed to start martin");

    martin.stop().await;
    for var in ["DATABASE_URL", "DEFAULT_SRID"] {
        martin.assert_log_contains(&format!("Environment variable {var} is deprecated"));
    }
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn every_kind_of_source_in_the_database_is_published() {
    let mut martin = martin_with_postgres().await;

    let catalog = martin.get("/catalog").await;
    assert_eq!(catalog.status(), 200);
    insta::assert_snapshot!(catalog.headers_snapshot_masking_etag(), @"
    content-encoding: br
    content-type: application/json
    etag: [ETAG]
    transfer-encoding: chunked
    vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    ");
    insta::assert_json_snapshot!("catalog", catalog.json()["tiles"]);

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn a_table_source_serves_tilejson_and_tiles_across_zooms() {
    let mut martin = martin_with_postgres().await;

    let table_source = tilejson(&martin, "/table_source").await;
    insta::assert_json_snapshot!(table_source, @r#"
    {
      "bounds": [
        -2.0,
        -1.0,
        142.8413150987,
        45.0
      ],
      "foo": {
        "bar": "foo"
      },
      "name": "table_source",
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/table_source/{z}/{x}/{y}"
      ],
      "vector_layers": [
        {
          "fields": {
            "gid": "int4"
          },
          "id": "table_source"
        }
      ]
    }
    "#);
    assert_tiles_across_zooms(&martin, "table_source", "table_source").await;

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn a_composite_source_serves_every_layer_it_names() {
    let mut martin = martin_with_postgres().await;

    let composite = tilejson(&martin, "/table_source,points1,points2").await;
    insta::assert_json_snapshot!("composite_tilejson", composite);
    assert_tiles_across_zooms(&martin, "table_source,points1,points2", "composite").await;

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn a_function_source_serves_tilejson_and_tiles_across_zooms() {
    let mut martin = martin_with_postgres().await;

    let function_source = tilejson(&martin, "/function_zxy_query").await;
    insta::assert_json_snapshot!(function_source, @r#"
    {
      "foo": {
        "bar": "foo"
      },
      "name": "function_zxy_query",
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/function_zxy_query/{z}/{x}/{y}"
      ]
    }
    "#);
    assert_tiles_across_zooms(&martin, "function_zxy_query", "function_zxy_query").await;

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn a_function_source_reads_its_query_string_and_can_return_a_raster() {
    let mut martin = martin_with_postgres().await;

    let with_token = tilejson(&martin, "/function_zxy_query_test").await;
    let with_jsonb = tilejson(&martin, "/function_zxy_query_jsonb").await;
    let returning_raster = tilejson(&martin, "/function_zxy_raster").await;
    insta::assert_json_snapshot!("function_with_token_tilejson", with_token);
    insta::assert_json_snapshot!("function_with_jsonb_tilejson", with_jsonb);
    insta::assert_json_snapshot!("function_returning_raster_tilejson", returning_raster);

    insta::assert_snapshot!(
        "function_with_token_0_0_0",
        tile_dump(&martin, "/function_zxy_query_test/0/0/0?token=martin").await
    );
    insta::assert_snapshot!(
        "function_with_jsonb_6_57_29",
        tile_dump(&martin, "/function_zxy_query_jsonb/6/57/29").await
    );

    let raster = martin.get("/function_zxy_raster/0/0/0").await;
    assert_eq!(raster.status(), 200);
    assert_eq!(raster.header("content-type"), Some("image/png"));
    assert_eq!(raster.image_size(), (1, 1));

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn every_function_calling_convention_serves_the_same_tile() {
    let mut martin = martin_with_postgres().await;

    // The fixture database declares one function per way martin may call it: positional zoom/x/y,
    // a row-returning variant, one that also returns a key, and a mixed-case name.
    for function in [
        "function_zoom_xy",
        "function_zxy",
        "function_zxy2",
        "function_zxy_query",
        "function_zxy_row",
        "function_Mixed_Name",
        "function_zxy_row_key",
    ] {
        let dump = tile_dump(&martin, &format!("/{function}/6/57/29")).await;
        insta::assert_snapshot!(format!("{function}_6_57_29"), dump);
    }

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn a_function_returning_a_key_column_serves_it_as_the_etag() {
    let mut martin = martin_with_postgres().await;

    let response = martin.get("/function_zxy_row_key/6/57/29").await;
    assert_eq!(response.status(), 200);
    // the function's `key` column, `md5(mvt)`, not a hash Martin computed
    let etag = response
        .header("etag")
        .expect("a keyed function tile must carry an etag")
        .to_owned();
    insta::assert_snapshot!(etag, @r#""2cab831e0c201dcbd5f081954ab45562""#);

    let cached = martin
        .get_with_headers("/function_zxy_row_key/6/57/29", &[("if-none-match", &etag)])
        .await;
    assert_eq!(cached.status(), 304);

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn a_curve_column_is_linearized_before_encoding() {
    let mut martin = martin_with_postgres().await;

    // A typed curve column and an untyped column holding curves both keep the linearization,
    // which is the only way ST_AsMVTGeom can encode them at all.
    insta::assert_snapshot!("curves_0_0_0", tile_dump(&martin, "/curves/0/0/0").await);
    insta::assert_snapshot!(
        "curves_untyped_0_0_0",
        tile_dump(&martin, "/curves_untyped/0/0/0").await
    );

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn a_table_keeps_its_own_srid_and_one_without_a_srid_gets_the_default() {
    let mut martin = martin_with_postgres().await;

    let srid_3857 = tilejson(&martin, "/points3857").await;
    insta::assert_json_snapshot!("srid_3857_tilejson", srid_3857);
    insta::assert_snapshot!(
        "srid_3857_0_0_0",
        tile_dump(&martin, "/points3857/0/0/0").await
    );
    // points_empty_srid has SRID 0 in the database and is only published because of --default-srid.
    insta::assert_snapshot!(
        "default_srid_0_0_0",
        tile_dump(&martin, "/points_empty_srid/0/0/0").await
    );

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn a_geometry_crossing_the_antimeridian_is_served_on_both_sides() {
    let mut martin = martin_with_postgres().await;

    insta::assert_snapshot!(
        "antimeridian_4_0_4",
        tile_dump(&martin, "/antimeridian/4/0/4").await
    );
    insta::assert_snapshot!(
        "antimeridian_4_0_5",
        tile_dump(&martin, "/antimeridian/4/0/5").await
    );

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn a_sql_comment_becomes_the_tilejson() {
    let mut martin = martin_with_postgres().await;

    let table_comment = tilejson(&martin, "/MixPoints").await;
    let function_comment = tilejson(&martin, "/function_Mixed_Name").await;
    insta::assert_json_snapshot!("table_comment_tilejson", table_comment);
    insta::assert_json_snapshot!("function_comment_tilejson", function_comment);

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn a_materialized_view_is_published_like_a_table() {
    let mut martin = martin_with_postgres().await;

    let materialized_view = tilejson(&martin, "/mat_view").await;
    insta::assert_json_snapshot!("materialized_view_tilejson", materialized_view);
    insta::assert_snapshot!(
        "materialized_view_0_0_0",
        tile_dump(&martin, "/mat_view/0/0/0").await
    );

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn the_same_name_in_two_schemas_gets_a_suffixed_id() {
    let mut martin = martin_with_postgres().await;

    // The second source of each pair is the one auto-discovery had to rename; the suffix is what
    // the `Source was renamed` warnings announce.
    for source in [
        "table_name_existing_two_schemas",
        "table_name_existing_two_schemas.1",
        "view_name_existing_two_schemas",
        "view_name_existing_two_schemas.1",
        "table_and_view_two_schemas",
        "table_and_view_two_schemas.1",
    ] {
        let snapshot_name = source.replace('.', "_dot_");
        let source_tilejson = tilejson(&martin, &format!("/{source}")).await;
        insta::assert_json_snapshot!(format!("{snapshot_name}_tilejson"), source_tilejson);
        let dump = tile_dump(&martin, &format!("/{source}/0/0/0")).await;
        insta::assert_snapshot!(format!("{snapshot_name}_0_0_0"), dump);
    }

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn a_config_file_publishes_what_it_names_and_what_auto_publish_adds() {
    let mut martin = martin_from_the_config().await;

    let catalog = martin.get("/catalog").await;
    assert_eq!(catalog.status(), 200);
    insta::assert_json_snapshot!(catalog.json()["tiles"], @r#"
    {
      "MixPoints": {
        "content_type": "application/x-protobuf",
        "description": "a description from comment on table"
      },
      "auto_table": {
        "content_type": "application/x-protobuf",
        "description": "autodetect.auto_table.geom"
      },
      "bigint_table": {
        "content_type": "application/x-protobuf",
        "description": "autodetect.bigint_table.geom"
      },
      "function_Mixed_Name": {
        "content_type": "application/x-protobuf",
        "description": "a function source with MixedCase name"
      },
      "function_zxy_query": {
        "content_type": "application/x-protobuf"
      },
      "function_zxy_query_test": {
        "content_type": "application/x-protobuf",
        "description": "public.function_zxy_query_test"
      },
      "points1": {
        "content_type": "application/x-protobuf",
        "description": "public.points1.geom"
      },
      "points1_filtered": {
        "content_type": "application/x-protobuf",
        "description": "public.points1.geom"
      },
      "points2": {
        "content_type": "application/x-protobuf",
        "description": "public.points2.geom"
      },
      "points3857": {
        "content_type": "application/x-protobuf",
        "description": "public.points3857.geom"
      },
      "table_source": {
        "content_type": "application/x-protobuf"
      }
    }
    "#);

    martin.stop().await;
    assert_unindexed_table_warnings(&mut martin);
}

#[tokio::test]
async fn a_configured_table_keeps_the_bounds_and_zoom_range_it_declares() {
    let mut martin = martin_from_the_config().await;

    let table_source = tilejson(&martin, "/table_source").await;
    insta::assert_json_snapshot!(table_source, @r#"
    {
      "bounds": [
        -180.0,
        -90.0,
        180.0,
        90.0
      ],
      "foo": {
        "bar": "foo"
      },
      "maxzoom": 30,
      "minzoom": 0,
      "name": "table_source",
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/table_source/{z}/{x}/{y}"
      ],
      "vector_layers": [
        {
          "fields": {
            "gid": "int4"
          },
          "id": "table_source"
        }
      ]
    }
    "#);
    insta::assert_snapshot!(
        "configured_table_source_0_0_0",
        tile_dump(&martin, "/table_source/0/0/0").await
    );

    martin.stop().await;
    assert_unindexed_table_warnings(&mut martin);
}

#[tokio::test]
async fn a_configured_composite_source_names_each_layer_after_its_layer_id() {
    let mut martin = martin_from_the_config().await;

    let composite = tilejson(&martin, "/table_source,points1,points2").await;
    insta::assert_json_snapshot!(composite, @r#"
    {
      "bounds": [
        -180.0,
        -90.0,
        180.0,
        90.0
      ],
      "description": "public.points1.geom\npublic.points2.geom",
      "maxzoom": 30,
      "minzoom": 0,
      "name": "table_source,points1,points2",
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/table_source,points1,points2/{z}/{x}/{y}"
      ],
      "vector_layers": [
        {
          "fields": {
            "gid": "int4"
          },
          "id": "table_source"
        },
        {
          "fields": {
            "gid": "int4"
          },
          "id": "abc"
        },
        {
          "fields": {
            "gid": "int4"
          },
          "id": "points2"
        }
      ]
    }
    "#);
    insta::assert_snapshot!(
        "configured_composite_0_0_0",
        tile_dump(&martin, "/table_source,points1,points2/0/0/0").await
    );

    martin.stop().await;
    assert_unindexed_table_warnings(&mut martin);
}

#[tokio::test]
async fn a_configured_function_serves_tiles_and_reads_its_query_string() {
    let mut martin = martin_from_the_config().await;

    insta::assert_snapshot!(
        "configured_function_zxy_query_0_0_0",
        tile_dump(&martin, "/function_zxy_query/0/0/0").await
    );
    insta::assert_snapshot!(
        "configured_function_with_token_0_0_0",
        tile_dump(&martin, "/function_zxy_query_test/0/0/0?token=martin").await
    );

    martin.stop().await;
    assert_unindexed_table_warnings(&mut martin);
}

#[tokio::test]
async fn a_source_configured_in_the_wrong_case_still_resolves_and_keeps_its_sql_comment() {
    let mut martin = martin_from_the_config().await;

    let table_comment = tilejson(&martin, "/MixPoints").await;
    let function_comment = tilejson(&martin, "/function_Mixed_Name").await;
    insta::assert_json_snapshot!(table_comment, @r#"
    {
      "bounds": [
        -170.9498596191,
        -84.2002563477,
        167.7089385986,
        74.2357330322
      ],
      "description": "a description from comment on table",
      "name": "MixPoints",
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/MixPoints/{z}/{x}/{y}"
      ],
      "vector_layers": [
        {
          "fields": {
            "Gid": "int4",
            "TABLE": "text"
          },
          "id": "MixPoints"
        }
      ]
    }
    "#);
    insta::assert_json_snapshot!(function_comment, @r#"
    {
      "description": "a function source with MixedCase name",
      "name": "function_Mixed_Name",
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/function_Mixed_Name/{z}/{x}/{y}"
      ],
      "vector_layers": [
        {
          "fields": {
            "Geom": "",
            "TABLE": ""
          },
          "id": "MixedCase.function_Mixed_Name"
        }
      ]
    }
    "#);

    martin.stop().await;
    assert_unindexed_table_warnings(&mut martin);
}

#[tokio::test]
async fn the_saved_config_spells_out_every_table_and_function_that_was_discovered() {
    let dir = tempfile::tempdir().expect("failed to create a temp dir");
    let save_config = dir.path().join("save_config.yaml");
    let mut martin =
        start_with_postgres(Martin::builder().arg("--save-config").arg(&save_config)).await;

    let saved = fs::read_to_string(&save_config).expect("martin did not write --save-config");
    insta::with_settings!({filters => vec![
        (r"(?m)^  connection_string: .*$", "  connection_string: [DATABASE_URL]"),
        (r"(-?\d+\.\d{10})\d+", "$1"),
    ]}, {
        insta::assert_snapshot!(saved);
    });

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn the_saved_config_carries_the_auto_publish_settings_into_every_table_it_adopts() {
    let dir = tempfile::tempdir().expect("failed to create a temp dir");
    let save_config = dir.path().join("save_config.yaml");
    let mut martin =
        start_from_the_config(Martin::builder().arg("--save-config").arg(&save_config)).await;

    let saved = fs::read_to_string(&save_config).expect("martin did not write --save-config");
    insta::with_settings!({filters => vec![
        (r"(?m)^  connection_string: .*$", "  connection_string: [DATABASE_URL]"),
        (r"(-?\d+\.\d{10})\d+", "$1"),
    ]}, {
        insta::assert_snapshot!(saved);
    });

    martin.stop().await;
    assert_unindexed_table_warnings(&mut martin);
}

#[tokio::test]
async fn an_auto_discovered_table_takes_the_connection_level_cache_bounds() {
    let mut bounded = Martin::builder()
        .with_postgres()
        .config(
            "
postgres:
  connection_string: ${DATABASE_URL}
  default_srid: 900913
  auto_bounds: calc
  pool_size: 1
  cache:
    minzoom: 1",
        )
        .start()
        .await
        .expect("failed to start martin");
    for _ in 0..2 {
        assert_eq!(bounded.get("/table_source/0/0/0").await.status(), 200);
    }
    let metrics = bounded.get("/_/metrics").await;
    assert_eq!(metrics.status(), 200);
    let tile_cache_lines = metrics
        .text()
        .lines()
        .filter(|line| line.starts_with("martin_tile_cache_requests_total"))
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(tile_cache_lines, @"");
    bounded.stop().await;
    assert_discovery_warnings(&mut bounded);
}

#[tokio::test]
async fn an_auto_discovered_table_takes_the_default_cache_bounds() {
    let mut unbounded = Martin::builder()
        .with_postgres()
        .config(
            "
postgres:
  connection_string: ${DATABASE_URL}
  default_srid: 900913
  auto_bounds: calc
  pool_size: 1",
        )
        .start()
        .await
        .expect("failed to start martin");
    for _ in 0..2 {
        assert_eq!(unbounded.get("/table_source/0/0/0").await.status(), 200);
    }
    let metrics = unbounded.get("/_/metrics").await;
    assert_eq!(metrics.status(), 200);
    let tile_cache_lines = metrics
        .text()
        .lines()
        .filter(|line| line.starts_with("martin_tile_cache_requests_total"))
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(tile_cache_lines, @r#"
    martin_tile_cache_requests_total{cache="tile",result="hit",zoom="0"} 1
    martin_tile_cache_requests_total{cache="tile",result="miss",zoom="0"} 1
    "#);
    unbounded.stop().await;
    assert_discovery_warnings(&mut unbounded);
}

#[tokio::test]
async fn a_function_returning_gzip_compressed_tiles_is_served_in_the_encoding_the_client_accepts() {
    let mut martin = martin_with_postgres().await;

    // function_zxy_gzip returns the tile function_zxy produces for 6/57/29, gzip-compressed.
    let expected = tile_dump(&martin, "/function_zxy/6/57/29").await;

    let compressed = martin.get("/function_zxy_gzip/6/57/29").await;
    assert_eq!(compressed.status(), 200);
    insta::assert_snapshot!(compressed.headers_snapshot_masking_etag(), @"
    content-encoding: gzip
    content-length: 78
    content-type: application/x-protobuf
    etag: [ETAG]
    vary: Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    ");
    assert_eq!(compressed.mvt_dump(), expected);

    // The harness always advertises gzip, so ask without it directly to see the tile decompressed.
    let plain = reqwest::Client::new()
        .get(format!(
            "http://{}/function_zxy_gzip/6/57/29",
            martin.addr()
        ))
        .header("accept-encoding", "identity")
        .send()
        .await
        .expect("request failed");
    assert_eq!(plain.status(), 200);
    assert_eq!(plain.headers().get("content-encoding"), None);
    assert_eq!(
        plain.bytes().await.expect("body failed").as_ref(),
        compressed.body()
    );

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn a_queryless_and_a_json_variant_route_by_the_query_string() {
    let mut martin = martin_with_postgres().await;

    // function_pair_json hands off to function_zxy without a query string and to
    // function_zxy_query with one, so the layer name says which variant ran.
    insta::assert_snapshot!(tile_dump(&martin, "/function_pair_json/6/57/29").await, @"
    layer: 0
      name: public.function_zxy
      version: 2
      extent: 4096
      feature: 0
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 1
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 2
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 3
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 4
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 5
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 6
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 7
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 8
        id: (none)
        geometry: POINT(1613,3539)
        properties: (none)
      feature: 9
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
    ");
    insta::assert_snapshot!(tile_dump(&martin, "/function_pair_json/6/57/29?answer=42").await, @"
    layer: 0
      name: public.function_zxy_query
      version: 2
      extent: 4096
      feature: 0
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 1
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 2
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 3
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 4
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 5
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 6
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 7
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 8
        id: (none)
        geometry: POINT(1613,3539)
        properties: (none)
      feature: 9
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
    ");

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn a_queryless_and_a_jsonb_variant_route_by_the_query_string() {
    let mut martin = martin_with_postgres().await;

    insta::assert_snapshot!(tile_dump(&martin, "/function_pair_jsonb/6/57/29").await, @"
    layer: 0
      name: public.function_zxy
      version: 2
      extent: 4096
      feature: 0
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 1
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 2
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 3
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 4
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 5
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 6
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 7
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 8
        id: (none)
        geometry: POINT(1613,3539)
        properties: (none)
      feature: 9
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
    ");
    insta::assert_snapshot!(tile_dump(&martin, "/function_pair_jsonb/6/57/29?answer=42").await, @"
    layer: 0
      name: public.function_zxy_query_jsonb
      version: 2
      extent: 4096
      feature: 0
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 1
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 2
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 3
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 4
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 5
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 6
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 7
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 8
        id: (none)
        geometry: POINT(1613,3539)
        properties: (none)
      feature: 9
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
    ");

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn a_json_and_a_jsonb_variant_are_two_sources() {
    let mut martin = martin_with_postgres().await;

    // Neither variant can take the bare URL from the other, so each keeps its own id.
    insta::assert_json_snapshot!(tilejson(&martin, "/function_pair_query").await, @r#"
    {
      "description": "public.function_pair_query",
      "name": "function_pair_query",
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/function_pair_query/{z}/{x}/{y}"
      ]
    }
    "#);
    insta::assert_json_snapshot!(tilejson(&martin, "/function_pair_query.1").await, @r#"
    {
      "description": "public.function_pair_query(integer, integer, integer, jsonb)",
      "name": "function_pair_query.1",
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/function_pair_query.1/{z}/{x}/{y}"
      ]
    }
    "#);
    insta::assert_snapshot!(tile_dump(&martin, "/function_pair_query/6/57/29").await, @"
    layer: 0
      name: public.function_zxy_query
      version: 2
      extent: 4096
      feature: 0
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 1
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 2
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 3
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 4
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 5
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 6
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 7
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 8
        id: (none)
        geometry: POINT(1613,3539)
        properties: (none)
      feature: 9
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
    ");
    insta::assert_snapshot!(tile_dump(&martin, "/function_pair_query.1/6/57/29").await, @"
    layer: 0
      name: public.function_zxy_query_jsonb
      version: 2
      extent: 4096
      feature: 0
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 1
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 2
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 3
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 4
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 5
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 6
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 7
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 8
        id: (none)
        geometry: POINT(1613,3539)
        properties: (none)
      feature: 9
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
    ");

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn the_comments_of_three_variants_land_where_their_source_is() {
    let mut martin = martin_with_postgres().await;

    // function_dup has all three variants, each with its own comment. The queryless and json
    // ones share a source, so their comments merge with the json one winning, and the jsonb
    // one keeps its comment on its own source.
    insta::assert_json_snapshot!(tilejson(&martin, "/function_dup").await, @r#"
    {
      "attribution": "from the queryless comment",
      "description": "the json variant",
      "name": "function_dup",
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/function_dup/{z}/{x}/{y}"
      ]
    }
    "#);
    insta::assert_json_snapshot!(tilejson(&martin, "/function_dup.1").await, @r#"
    {
      "description": "the jsonb variant",
      "name": "function_dup.1",
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/function_dup.1/{z}/{x}/{y}"
      ]
    }
    "#);
    insta::assert_snapshot!(tile_dump(&martin, "/function_dup/6/57/29").await, @"
    layer: 0
      name: public.function_zxy
      version: 2
      extent: 4096
      feature: 0
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 1
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 2
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 3
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 4
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 5
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 6
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 7
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 8
        id: (none)
        geometry: POINT(1613,3539)
        properties: (none)
      feature: 9
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
    ");
    insta::assert_snapshot!(tile_dump(&martin, "/function_dup/6/57/29?answer=42").await, @"
    layer: 0
      name: public.function_zxy_query
      version: 2
      extent: 4096
      feature: 0
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 1
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 2
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 3
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 4
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 5
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 6
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 7
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 8
        id: (none)
        geometry: POINT(1613,3539)
        properties: (none)
      feature: 9
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
    ");
    insta::assert_snapshot!(tile_dump(&martin, "/function_dup.1/6/57/29").await, @"
    layer: 0
      name: public.function_zxy_query_jsonb
      version: 2
      extent: 4096
      feature: 0
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 1
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 2
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 3
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 4
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 5
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 6
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 7
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 8
        id: (none)
        geometry: POINT(1613,3539)
        properties: (none)
      feature: 9
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
    ");

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn the_same_function_name_in_two_schemas_keeps_each_comment() {
    let mut martin = martin_with_postgres().await;

    insta::assert_json_snapshot!(tilejson(&martin, "/function_two_schemas").await, @r#"
    {
      "description": "the schema_a comment",
      "name": "function_two_schemas",
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/function_two_schemas/{z}/{x}/{y}"
      ]
    }
    "#);
    insta::assert_json_snapshot!(tilejson(&martin, "/function_two_schemas.1").await, @r#"
    {
      "description": "the schema_b comment",
      "name": "function_two_schemas.1",
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/function_two_schemas.1/{z}/{x}/{y}"
      ]
    }
    "#);

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
}

#[tokio::test]
async fn a_configured_function_names_a_further_variant_by_its_signature() {
    let mut martin = Martin::builder()
        .with_postgres()
        .config(
            "
postgres:
  connection_string: ${DATABASE_URL}
  pool_size: 1
  functions:
    pair:
      schema: public
      function: function_dup
    with_jsonb:
      schema: public
      function: function_dup(integer, integer, integer, jsonb)
",
        )
        .start()
        .await
        .expect("failed to start martin");

    insta::assert_snapshot!(tile_dump(&martin, "/pair/6/57/29").await, @"
    layer: 0
      name: public.function_zxy
      version: 2
      extent: 4096
      feature: 0
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 1
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 2
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 3
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 4
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 5
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 6
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 7
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 8
        id: (none)
        geometry: POINT(1613,3539)
        properties: (none)
      feature: 9
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
    ");
    insta::assert_snapshot!(tile_dump(&martin, "/pair/6/57/29?answer=42").await, @"
    layer: 0
      name: public.function_zxy_query
      version: 2
      extent: 4096
      feature: 0
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 1
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 2
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 3
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 4
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 5
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 6
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 7
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 8
        id: (none)
        geometry: POINT(1613,3539)
        properties: (none)
      feature: 9
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
    ");
    insta::assert_snapshot!(tile_dump(&martin, "/with_jsonb/6/57/29").await, @"
    layer: 0
      name: public.function_zxy_query_jsonb
      version: 2
      extent: 4096
      feature: 0
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 1
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 2
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 3
        id: (none)
        geometry: POINT(1614,3540)
        properties: (none)
      feature: 4
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 5
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
      feature: 6
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 7
        id: (none)
        geometry: POINT(1613,3540)
        properties: (none)
      feature: 8
        id: (none)
        geometry: POINT(1613,3539)
        properties: (none)
      feature: 9
        id: (none)
        geometry: POINT(1614,3539)
        properties: (none)
    ");

    martin.stop().await;
}

#[tokio::test]
async fn a_cql2_filter_limits_the_rows_a_table_serves_and_its_bounds() {
    let mut martin = martin_from_the_config().await;

    // points1 holds 30 points with gid 1..=30 and the config keeps the first three.
    let filtered = tilejson(&martin, "/points1_filtered").await;
    insta::assert_json_snapshot!(filtered, @r#"
    {
      "bounds": [
        142.8404063069,
        11.926741846,
        142.8414336,
        11.927383336
      ],
      "description": "public.points1.geom",
      "name": "points1_filtered",
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/points1_filtered/{z}/{x}/{y}"
      ],
      "vector_layers": [
        {
          "fields": {
            "gid": "int4"
          },
          "id": "filtered"
        }
      ]
    }
    "#);
    insta::assert_snapshot!(tile_dump(&martin, "/points1_filtered/0/0/0").await, @"
    layer: 0
      name: filtered
      version: 2
      extent: 4096
      feature: 0
        id: (none)
        geometry: POINT(3673,1911)
        properties:
          gid = 3 (uint)
      feature: 1
        id: (none)
        geometry: POINT(3673,1911)
        properties:
          gid = 2 (uint)
      feature: 2
        id: (none)
        geometry: POINT(3673,1911)
        properties:
          gid = 1 (uint)
    ");

    martin.stop().await;
    assert_unindexed_table_warnings(&mut martin);
}

#[tokio::test]
async fn a_filter_that_is_not_cql2_stops_martin_at_startup() {
    let error = Martin::builder()
        .with_postgres()
        .env("RUST_LOG", "martin=error")
        .config(
            "
postgres:
  connection_string: ${DATABASE_URL}
  tables:
    broken:
      schema: public
      table: points1
      srid: 4326
      geometry_column: geom
      filter: gid <=
",
        )
        .start()
        .await
        .expect_err("martin must not start with a filter it cannot parse");
    let StartError::EarlyExit { status, log } = error else {
        panic!("expected an early exit, got: {error}");
    };
    assert!(!status.success(), "exit status must be a failure: {status}");
    insta::assert_snapshot!(log, @"
    ERROR Filter 'gid <=' is not valid CQL2:  --> 1:7
      |
    1 | gid <=
      |       ^---
      |
      = expected GEOMETRY, Identifier, Negative, UnaryNot, True, False, Null, DECIMAL, Double, SingleQuotedString, ExpressionInParentheses, or Array
    ");
}

/// Three tables on three grids.
/// New Zealand on LINZ's NZTM2000Quad, world points on a square WGS84 grid, and Mars landing sites on a CRS PostGIS only knows from a `spatial_ref_sys` row.
const TILE_GRIDS_CONFIG: &str = "
tile_grids:
  NZTM2000Quad:
    crs: EPSG:2193
    origin: [-3260586.7284, 10438190.1652]
    extent_at_zoom0: 10018754.1714
  WGS84Square:
    crs: EPSG:4326
    origin: [-180, 90]
    extent_at_zoom0: 360
  MarsGeographic:
    crs: IAU_2015:49900
    origin: [-180, 90]
    extent_at_zoom0: 360
  FloorPlan:
    crs: simple
    origin: [0, 1000]
    extent_at_zoom0: 1000
mbtiles:
  sources:
    cities_nztm:
      path: WORLD_CITIES_MBTILES
      tile_grid: NZTM2000Quad
postgres:
  connection_string: ${DATABASE_URL}
  pool_size: 1
  tables:
    nz_points:
      schema: public
      table: nz_points
      srid: 2193
      geometry_column: geom
      tile_grid: NZTM2000Quad
      properties:
        city: text
    points1_wgs84:
      schema: public
      table: points1
      srid: 4326
      geometry_column: geom
      tile_grid: WGS84Square
      properties:
        gid: int4
    points1:
      schema: public
      table: points1
      srid: 4326
      geometry_column: geom
      properties:
        gid: int4
    mars_points:
      schema: public
      table: mars_points
      srid: 949900
      geometry_column: geom
      tile_grid: MarsGeographic
      properties:
        site: text
    points1_crs84:
      schema: public
      table: points1
      srid: 4326
      geometry_column: geom
      tile_grid: WorldCRS84Quad
      properties:
        gid: int4
    points1_nztm:
      schema: public
      table: points1
      srid: 4326
      geometry_column: geom
      tile_grid: NZTM2000Quad
      properties:
        gid: int4
    floor_plan:
      schema: public
      table: floor_plan
      srid: 0
      geometry_column: geom
      tile_grid: FloorPlan
      properties:
        room: text
";

/// The archive in [`TILE_GRIDS_CONFIG`] is a copy of the `world_cities` fixture, kept alive by the returned directory.
async fn martin_with_tile_grids() -> (Martin, TempDir) {
    let dir = temp_dir();
    let archive = mbtiles_fixture(dir.path(), "world_cities").await;
    let config = TILE_GRIDS_CONFIG.replace("WORLD_CITIES_MBTILES", &archive.display().to_string());
    let martin = Martin::builder()
        .with_postgres()
        .config(&config)
        .start()
        .await
        .expect("failed to start martin");
    (martin, dir)
}

/// The one warning every start from [`TILE_GRIDS_CONFIG`] logs, because Mars has no WGS84 bounds.
fn assert_tile_grid_warnings(martin: &mut Martin) {
    martin.assert_log_contains(
        "Not computing the bounds of public.mars_points.geom: SRID 949900 is IAU_2015:49900, not an EPSG system",
    );
}

#[tokio::test]
async fn a_table_on_another_tile_grid_advertises_the_grid_and_serves_its_tiles() {
    let (mut martin, _dir) = martin_with_tile_grids().await;

    let nz = tilejson(&martin, "/nz_points").await;
    insta::assert_json_snapshot!(nz, @r#"
    {
      "bounds": [
        172.6287183595,
        -43.5323403824,
        174.7610537778,
        -36.8528850812
      ],
      "description": "public.nz_points.geom",
      "name": "nz_points",
      "tileGrid": {
        "crs": "EPSG:2193",
        "extentAtZoom0": 10018754.1714,
        "id": "NZTM2000Quad",
        "origin": [
          -3260586.7284,
          10438190.1652
        ]
      },
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/nz_points/{z}/{x}/{y}"
      ],
      "vector_layers": [
        {
          "fields": {
            "city": "text"
          },
          "id": "nz_points"
        }
      ]
    }
    "#);
    // the three cities sit in three different zoom-2 tiles of the grid, and nowhere else
    for zxy in ["2/2/1", "2/2/2", "2/1/2"] {
        let dump = tile_dump(&martin, &format!("/nz_points/{zxy}")).await;
        insta::assert_snapshot!(format!("nztm2000quad_{}", zxy.replace('/', "_")), dump);
    }
    assert_eq!(martin.get("/nz_points/2/0/0").await.status(), 204);

    let catalog = martin.get("/catalog").await.json();
    assert_eq!(catalog["tiles"]["nz_points"]["tile_grid"], "NZTM2000Quad");
    assert_eq!(catalog["tiles"]["points1"].get("tile_grid"), None);

    martin.stop().await;
    assert_tile_grid_warnings(&mut martin);
}

#[tokio::test]
async fn the_same_table_on_two_grids_splits_the_world_differently() {
    let (mut martin, _dir) = martin_with_tile_grids().await;

    // on the square WGS84 grid, zoom 1 splits the world at the equator and the prime meridian
    for zxy in ["1/0/0", "1/1/0"] {
        let dump = tile_dump(&martin, &format!("/points1_wgs84/{zxy}")).await;
        insta::assert_snapshot!(
            format!("wgs84square_points1_{}", zxy.replace('/', "_")),
            dump
        );
    }
    // the lower half of the square is below the south pole, so those tiles are empty
    assert_eq!(martin.get("/points1_wgs84/1/0/1").await.status(), 204);

    // a composite source cannot mix grids
    let mixed = martin.get("/points1_wgs84,points1/1/0/0").await;
    assert_eq!(mixed.status(), 400);
    assert_eq!(
        mixed.text(),
        "Cannot merge sources in tile grid WGS84Square with WebMercatorQuad"
    );

    martin.stop().await;
    assert_tile_grid_warnings(&mut martin);
    martin
        .assert_log_contains("Cannot merge sources in tile grid WGS84Square with WebMercatorQuad");
}

#[tokio::test]
async fn a_grid_outside_epsg_is_resolved_through_spatial_ref_sys() {
    let (mut martin, _dir) = martin_with_tile_grids().await;

    let mars = tilejson(&martin, "/mars_points").await;
    // no WGS84 bounds exist for Mars, so none are advertised
    assert_eq!(mars.get("bounds"), None);
    assert_eq!(mars["tileGrid"]["crs"], "IAU_2015:49900");
    for zxy in ["1/0/0", "1/1/0"] {
        let dump = tile_dump(&martin, &format!("/mars_points/{zxy}")).await;
        insta::assert_snapshot!(format!("mars_points_{}", zxy.replace('/', "_")), dump);
    }

    martin.stop().await;
    assert_tile_grid_warnings(&mut martin);
}

#[tokio::test]
async fn a_two_wide_grid_splits_the_world_at_the_prime_meridian() {
    let (mut martin, _dir) = martin_with_tile_grids().await;

    let tilejson = tilejson(&martin, "/points1_crs84").await;
    assert_eq!(tilejson["tileGrid"]["id"], "WorldCRS84Quad");
    assert_eq!(
        tilejson["tileGrid"]["matrixAtZoom0"],
        serde_json::json!([2, 1])
    );
    for zxy in ["0/0/0", "0/1/0"] {
        let dump = tile_dump(&martin, &format!("/points1_crs84/{zxy}")).await;
        insta::assert_snapshot!(
            format!("worldcrs84quad_points1_{}", zxy.replace('/', "_")),
            dump
        );
    }
    // there is no third column and no second row at zoom 0
    assert_eq!(martin.get("/points1_crs84/0/2/0").await.status(), 404);
    assert_eq!(martin.get("/points1_crs84/0/0/1").await.status(), 404);
    // and Web Mercator has always had exactly one tile at zoom 0
    assert_eq!(martin.get("/points1/0/1/0").await.status(), 404);

    martin.stop().await;
    assert_tile_grid_warnings(&mut martin);
    martin.assert_log_contains("is outside the WorldCRS84Quad grid of points1_crs84");
    martin.assert_log_contains("is outside the WebMercatorQuad grid of points1");
}

#[tokio::test]
async fn a_simple_grid_serves_plain_planar_coordinates() {
    let (mut martin, _dir) = martin_with_tile_grids().await;

    let tilejson = tilejson(&martin, "/floor_plan").await;
    assert_eq!(tilejson["tileGrid"]["crs"], "simple");
    assert_eq!(tilejson.get("bounds"), None);
    // zoom 1 quarters the 1000 by 1000 plan, one room per quarter
    for zxy in ["1/0/0", "1/1/0", "1/1/1"] {
        let dump = tile_dump(&martin, &format!("/floor_plan/{zxy}")).await;
        insta::assert_snapshot!(format!("floor_plan_{}", zxy.replace('/', "_")), dump);
    }
    assert_eq!(martin.get("/floor_plan/1/0/1").await.status(), 204);

    martin.stop().await;
    assert_tile_grid_warnings(&mut martin);
}

#[tokio::test]
async fn an_archive_can_be_declared_to_be_on_a_grid() {
    let (mut martin, _dir) = martin_with_tile_grids().await;

    let tilejson = tilejson(&martin, "/cities_nztm").await;
    assert_eq!(tilejson["tileGrid"]["id"], "NZTM2000Quad");
    let catalog = martin.get("/catalog").await.json();
    assert_eq!(catalog["tiles"]["cities_nztm"]["tile_grid"], "NZTM2000Quad");
    // the declaration changes nothing about the bytes, which pass straight through
    assert_eq!(martin.get("/cities_nztm/0/0/0").await.status(), 200);

    martin.stop().await;
    assert_tile_grid_warnings(&mut martin);
}

/// The zoom-0 tile of NZTM2000Quad reaches across 180 degrees, where longitude and latitude cut the world open.
/// Transformed into WGS84, its corners lie at both ends of the world, so a plain bounding box of them lands on the wrong side and misses the strip next to the cut.
/// `points1` has a point at 175.7 degrees west inside that strip.
#[tokio::test]
async fn a_table_in_wgs84_keeps_its_features_next_to_the_antimeridian() {
    let (mut martin, _dir) = martin_with_tile_grids().await;

    let dump = tile_dump(&martin, "/points1_nztm/0/0/0").await;
    insta::assert_snapshot!("nztm2000quad_wgs84_table_0_0_0", dump);

    martin.stop().await;
    assert_tile_grid_warnings(&mut martin);
}

//! `PostgreSQL` sources: the ones martin discovers by itself, and the ones a config file declares.

#![cfg(feature = "test-pg")]

use std::fs;

use martin_integration_tests::{Martin, MartinBuilder, round_floats};
use serde_json::Value;

/// A server that publishes everything it finds in the fixture database.
async fn martin_with_postgres() -> Martin {
    Martin::builder()
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
        "source.id.new=table_source_multiple_geom.1",
        "source.id.new=table_name_existing_two_schemas.1",
        "source.id.new=view_name_existing_two_schemas.1",
        "source.id.new=table_and_view_two_schemas.1",
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
async fn every_kind_of_source_in_the_database_is_published() {
    let mut martin = martin_with_postgres().await;

    let catalog = martin.get("/catalog").await;
    assert_eq!(catalog.status(), 200);
    insta::assert_snapshot!(catalog.headers_snapshot(), @r"
    content-encoding: br
    content-type: application/json
    transfer-encoding: chunked
    vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    ");
    insta::assert_json_snapshot!("catalog", catalog.json()["tiles"]);

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
    martin.assert_log_clean();
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
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_composite_source_serves_every_layer_it_names() {
    let mut martin = martin_with_postgres().await;

    let composite = tilejson(&martin, "/table_source,points1,points2").await;
    insta::assert_json_snapshot!("composite_tilejson", composite);
    assert_tiles_across_zooms(&martin, "table_source,points1,points2", "composite").await;

    martin.stop().await;
    assert_discovery_warnings(&mut martin);
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
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
    martin.assert_log_clean();
}

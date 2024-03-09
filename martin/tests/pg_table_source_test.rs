#![cfg(feature = "postgres")]

use ctor::ctor;
use indoc::indoc;
use insta::assert_yaml_snapshot;
use martin::TileCoord;

pub mod utils;
pub use utils::*;

#[ctor]
fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[actix_rt::test]
async fn table_source() {
    let mock = mock_sources(mock_pgcfg("connection_string: $DATABASE_URL")).await;
    assert_yaml_snapshot!(mock.0.tiles.get_catalog(), @r###"
    ---
    "-function.withweired---_-characters":
      content_type: application/x-protobuf
      description: a function source with special characters
    ".-Points---quote":
      content_type: application/x-protobuf
      description: Escaping test table
    MixPoints:
      content_type: application/x-protobuf
      description: a description from comment on table
    auto_table:
      content_type: application/x-protobuf
      description: autodetect.auto_table.geom
    bigint_table:
      content_type: application/x-protobuf
      description: autodetect.bigint_table.geom
    function_Mixed_Name:
      content_type: application/x-protobuf
      description: a function source with MixedCase name
    function_null:
      content_type: application/x-protobuf
      description: public.function_null
    function_null_row:
      content_type: application/x-protobuf
      description: public.function_null_row
    function_null_row2:
      content_type: application/x-protobuf
      description: public.function_null_row2
    function_zoom_xy:
      content_type: application/x-protobuf
      description: public.function_zoom_xy
    function_zxy:
      content_type: application/x-protobuf
      description: public.function_zxy
    function_zxy2:
      content_type: application/x-protobuf
      description: public.function_zxy2
    function_zxy_query:
      content_type: application/x-protobuf
    function_zxy_query_jsonb:
      content_type: application/x-protobuf
      description: public.function_zxy_query_jsonb
    function_zxy_query_test:
      content_type: application/x-protobuf
      description: public.function_zxy_query_test
    function_zxy_row:
      content_type: application/x-protobuf
      description: public.function_zxy_row
    function_zxy_row_key:
      content_type: application/x-protobuf
      description: public.function_zxy_row_key
    points1:
      content_type: application/x-protobuf
      description: public.points1.geom
    points1_vw:
      content_type: application/x-protobuf
      description: public.points1_vw.geom
    points2:
      content_type: application/x-protobuf
      description: public.points2.geom
    points3857:
      content_type: application/x-protobuf
      description: public.points3857.geom
    table_source:
      content_type: application/x-protobuf
    table_source_multiple_geom:
      content_type: application/x-protobuf
      description: public.table_source_multiple_geom.geom1
    table_source_multiple_geom.1:
      content_type: application/x-protobuf
      description: public.table_source_multiple_geom.geom2
    "###);

    let source = table(&mock, "table_source");
    assert_yaml_snapshot!(source, @r###"
    ---
    schema: public
    table: table_source
    srid: 4326
    geometry_column: geom
    bounds:
      - -2
      - -1
      - 142.84131509869133
      - 45
    geometry_type: GEOMETRY
    properties:
      gid: int4
    "###);
}

#[actix_rt::test]
async fn tables_tilejson() {
    let mock = mock_sources(mock_pgcfg("connection_string: $DATABASE_URL")).await;
    let tj = source(&mock, "table_source").get_tilejson();
    assert_yaml_snapshot!(tj, @r###"
    ---
    tilejson: 3.0.0
    tiles: []
    vector_layers:
      - id: table_source
        fields:
          gid: int4
    bounds:
      - -2
      - -1
      - 142.84131509869133
      - 45
    name: table_source
    foo:
      bar: foo
    "###);
}

#[actix_rt::test]
async fn tables_tile_ok() {
    let mock = mock_sources(mock_pgcfg("connection_string: $DATABASE_URL")).await;
    let tile = source(&mock, "table_source")
        .get_tile(TileCoord { z: 0, x: 0, y: 0 }, None)
        .await
        .unwrap();

    assert!(!tile.is_empty());
}

#[actix_rt::test]
async fn tables_srid_ok() {
    let mock = mock_sources(mock_pgcfg(indoc! {"
        connection_string: $DATABASE_URL
        default_srid: 900913
    "}))
    .await;

    let source = table(&mock, "points1");
    assert_eq!(source.srid, 4326);

    let source = table(&mock, "points2");
    assert_eq!(source.srid, 4326);

    let source = table(&mock, "points3857");
    assert_eq!(source.srid, 3857);

    let source = table(&mock, "points_empty_srid");
    assert_eq!(source.srid, 900_913);
}

#[actix_rt::test]
async fn tables_multiple_geom_ok() {
    let mock = mock_sources(mock_pgcfg("connection_string: $DATABASE_URL")).await;

    let source = table(&mock, "table_source_multiple_geom");
    assert_eq!(source.geometry_column, "geom1");

    let source = table(&mock, "table_source_multiple_geom.1");
    assert_eq!(source.geometry_column, "geom2");
}

#[actix_rt::test]
async fn table_source_schemas() {
    let cfg = mock_pgcfg(indoc! {"
        connection_string: $DATABASE_URL
        auto_publish:
          tables:
            from_schemas: MixedCase
          functions: false
    "});
    let sources = mock_sources(cfg).await.0;
    assert_yaml_snapshot!(sources.tiles.get_catalog(), @r###"
    ---
    MixPoints:
      content_type: application/x-protobuf
      description: a description from comment on table
    "###);
}

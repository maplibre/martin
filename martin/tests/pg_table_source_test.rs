#![cfg(feature = "test-pg")]

use indoc::indoc;
use insta::assert_yaml_snapshot;
use martin_tile_utils::TileCoord;
pub mod utils;
pub use utils::*;

#[actix_rt::test]
async fn table_source() {
    let mock = mock_sources(mock_pgcfg("connection_string: $DATABASE_URL").await).await;
    insta::with_settings!({sort_maps => true}, {
    assert_yaml_snapshot!(mock.0.tile_manager.tile_sources().get_catalog(), @r#"
    "-function.withweired---_-characters":
      content_type: application/x-protobuf
      description: a function source with special characters
    ".-Points-----------quote":
      content_type: application/x-protobuf
      description: Escaping test table
    MixPoints:
      content_type: application/x-protobuf
      description: a description from comment on table
    antimeridian:
      content_type: application/x-protobuf
      description: public.antimeridian.geom
    auto_table:
      content_type: application/x-protobuf
      description: autodetect.auto_table.geom
    bigint_table:
      content_type: application/x-protobuf
      description: autodetect.bigint_table.geom
    curves:
      content_type: application/x-protobuf
      description: public.curves.geom
    curves_untyped:
      content_type: application/x-protobuf
      description: public.curves_untyped.geom
    empty_bounds:
      content_type: application/x-protobuf
      description: public.empty_bounds.geom
    function_Mixed_Name:
      content_type: application/x-protobuf
      description: a function source with MixedCase name
    function_dup:
      content_type: application/x-protobuf
      description: the json variant
      attribution: from the queryless comment
    function_dup.1:
      content_type: application/x-protobuf
      description: the jsonb variant
    function_null:
      content_type: application/x-protobuf
      description: public.function_null
    function_null_row:
      content_type: application/x-protobuf
      description: public.function_null_row
    function_null_row2:
      content_type: application/x-protobuf
      description: public.function_null_row2
    function_pair_json:
      content_type: application/x-protobuf
      description: public.function_pair_json
    function_pair_jsonb:
      content_type: application/x-protobuf
      description: public.function_pair_jsonb
    function_pair_query:
      content_type: application/x-protobuf
      description: public.function_pair_query
    function_pair_query.1:
      content_type: application/x-protobuf
      description: "public.function_pair_query(integer, integer, integer, jsonb)"
    function_two_schemas:
      content_type: application/x-protobuf
      description: the schema_a comment
    function_two_schemas.1:
      content_type: application/x-protobuf
      description: the schema_b comment
    function_zoom_xy:
      content_type: application/x-protobuf
      description: public.function_zoom_xy
    function_zxy:
      content_type: application/x-protobuf
      description: public.function_zxy
    function_zxy2:
      content_type: application/x-protobuf
      description: public.function_zxy2
    function_zxy_gzip:
      content_type: application/x-protobuf
      description: a function source returning gzip-compressed tiles
    function_zxy_query:
      content_type: application/x-protobuf
    function_zxy_query_jsonb:
      content_type: application/x-protobuf
      description: public.function_zxy_query_jsonb
    function_zxy_query_test:
      content_type: application/x-protobuf
      description: public.function_zxy_query_test
    function_zxy_raster:
      content_type: image/png
      description: a raster tile function source
    function_zxy_row:
      content_type: application/x-protobuf
      description: public.function_zxy_row
    function_zxy_row_key:
      content_type: application/x-protobuf
      description: public.function_zxy_row_key
    linestring_bounds:
      content_type: application/x-protobuf
      description: public.linestring_bounds.geom
    linestring_bounds_vertical:
      content_type: application/x-protobuf
      description: public.linestring_bounds_vertical.geom
    mars_points:
      content_type: application/x-protobuf
      description: public.mars_points.geom
    nz_points:
      content_type: application/x-protobuf
      description: public.nz_points.geom
    point_bounds:
      content_type: application/x-protobuf
      description: public.point_bounds.geom
    points1:
      content_type: application/x-protobuf
      description: public.points1.geom
    points1_vw:
      content_type: application/x-protobuf
      description: description from SQL comment
      attribution: some attribution from SQL comment
    points2:
      content_type: application/x-protobuf
      description: public.points2.geom
    points3857:
      content_type: application/x-protobuf
      description: public.points3857.geom
    table_name_existing_two_schemas:
      content_type: application/x-protobuf
      description: schema_a.table_name_existing_two_schemas.a_geom
    table_name_existing_two_schemas.1:
      content_type: application/x-protobuf
      description: schema_b.table_name_existing_two_schemas.b_geom
    table_source:
      content_type: application/x-protobuf
    table_source_geog:
      content_type: application/x-protobuf
    table_source_multiple_geom:
      content_type: application/x-protobuf
      description: public.table_source_multiple_geom.geom1
    table_source_multiple_geom.1:
      content_type: application/x-protobuf
      description: public.table_source_multiple_geom.geom2
    view_name_existing_two_schemas:
      content_type: application/x-protobuf
      description: schema_a.view_name_existing_two_schemas.a_geom
    view_name_existing_two_schemas.1:
      content_type: application/x-protobuf
      description: schema_b.view_name_existing_two_schemas.b_geom
    "#);
    });

    let source = table(&mock, "table_source");
    assert_yaml_snapshot!(source, @"
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
    ");

    let source2 = table(&mock, "table_source_geog");
    assert_yaml_snapshot!(source2, @"
    schema: public
    table: table_source_geog
    srid: 4326
    geometry_column: geog
    bounds:
      - -2
      - 0
      - 142.84131509869133
      - 45
    geometry_type: Geometry
    properties:
      gid: int4
    ");

    let source3 = table(&mock, "points3857");
    assert_yaml_snapshot!(source3, @"
    schema: public
    table: points3857
    srid: 3857
    geometry_column: geom
    bounds:
      - -161.4059125851273
      - -81.50727080755011
      - 172.51550346797322
      - 84.24401966908702
    geometry_type: POINT
    properties:
      gid: int4
    ");
}

#[actix_rt::test]
async fn tables_tilejson() {
    let mock = mock_sources(mock_pgcfg("connection_string: $DATABASE_URL").await).await;
    let src = source(&mock, "table_source");
    assert_yaml_snapshot!(src.get_tilejson(), @"
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
    ");
}

#[actix_rt::test]
async fn tables_tile_ok() {
    let mock = mock_sources(mock_pgcfg("connection_string: $DATABASE_URL").await).await;
    let tile = source(&mock, "table_source")
        .get_tile(TileCoord { z: 0, x: 0, y: 0 }, None)
        .await
        .unwrap();

    assert!(!tile.is_empty());
}

#[actix_rt::test]
async fn tables_srid_ok() {
    let mock = mock_sources(
        mock_pgcfg(indoc! {"
        connection_string: $DATABASE_URL
        default_srid: 900913
    "})
        .await,
    )
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
    let mock = mock_sources(mock_pgcfg("connection_string: $DATABASE_URL").await).await;

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
    "})
    .await;
    let sources = mock_sources(cfg).await.0;
    assert_yaml_snapshot!(sources.tile_manager.tile_sources().get_catalog(), @"
    MixPoints:
      content_type: application/x-protobuf
      description: a description from comment on table
    ");
}

#[actix_rt::test]
async fn table_bounds_linestring_horizontal_ok() {
    let mock = mock_sources(mock_pgcfg("connection_string: $DATABASE_URL").await).await;
    let source = table(&mock, "linestring_bounds");
    assert_yaml_snapshot!(source, @"
    schema: public
    table: linestring_bounds
    srid: 4326
    geometry_column: geom
    bounds:
      - 9.958169937133789
      - 10.037016868591309
      - 9.967533111572266
      - 10.037017822265625
    geometry_type: GEOMETRY
    properties:
      gid: int4
    ");
}

#[actix_rt::test]
async fn table_bounds_linestring_vertical_ok() {
    let mock = mock_sources(mock_pgcfg("connection_string: $DATABASE_URL").await).await;
    let source = table(&mock, "linestring_bounds_vertical");
    assert_yaml_snapshot!(source, @"
    schema: public
    table: linestring_bounds_vertical
    srid: 4326
    geometry_column: geom
    bounds:
      - 9
      - 8.958169937133789
      - 11
      - 10.967533111572266
    geometry_type: GEOMETRY
    properties:
      gid: int4
    ");
}

#[actix_rt::test]
async fn table_bounds_single_point_ok() {
    let mock = mock_sources(mock_pgcfg("connection_string: $DATABASE_URL").await).await;
    let source = table(&mock, "point_bounds");
    assert_yaml_snapshot!(source, @"
    schema: public
    table: point_bounds
    srid: 4326
    geometry_column: geom
    bounds:
      - 9
      - 19
      - 11
      - 21
    geometry_type: GEOMETRY
    properties:
      gid: int4
    ");
}

#[actix_rt::test]
async fn table_bounds_empty_table_ok() {
    let mock = mock_sources(mock_pgcfg("connection_string: $DATABASE_URL").await).await;
    let source = table(&mock, "empty_bounds");
    assert_yaml_snapshot!(source, @"
    schema: public
    table: empty_bounds
    srid: 4326
    geometry_column: geom
    geometry_type: GEOMETRY
    properties:
      gid: int4
    ");
}

#[actix_rt::test]
async fn tables_tile_grid_must_be_configured() {
    let yaml = indoc! {"
        tile_grids:
          NZTM2000Quad:
            crs: EPSG:2193
            origin: [-3260586.7284, 10438190.1652]
            extent_at_zoom0: 10018754.1714
        postgres:
          connection_string: $DATABASE_URL
          tables:
            nz_points:
              schema: public
              table: nz_points
              srid: 2193
              geometry_column: geom
              tile_grid: NZTM2000quad
    "};
    let env: martin::config::primitives::env::FauxEnv = std::env::var("DATABASE_URL")
        .map(|url| vec![("DATABASE_URL", url.into())].into_iter().collect())
        .unwrap_or_default();
    let mut cfg = martin::config::file::parse_config(
        yaml,
        &martin::config::primitives::env::Env::as_property_map(&env),
        std::path::Path::new("test.yaml"),
    )
    .expect("config parses");
    let err = cfg
        .finalize()
        .await
        .expect_err("an unknown tile grid is a config error");
    assert_eq!(
        err.to_string(),
        "Table source nz_points refers to tile grid NZTM2000quad, which is not configured. Known grids: NZTM2000Quad, WebMercatorQuad"
    );
}

#[actix_rt::test]
async fn tables_tile_grid_is_the_connection_default_unless_a_table_names_one() {
    let mock = mock_sources(
        mock_cfg(indoc! {"
        tile_grids:
          NZTM2000Quad:
            crs: EPSG:2193
            origin: [-3260586.7284, 10438190.1652]
            extent_at_zoom0: 10018754.1714
        postgres:
          connection_string: $DATABASE_URL
          tile_grid: NZTM2000Quad
          tables:
            nz_points:
              schema: public
              table: nz_points
              srid: 2193
              geometry_column: geom
            points1:
              schema: public
              table: points1
              srid: 4326
              geometry_column: geom
              tile_grid: WebMercatorQuad
    "})
        .await,
    )
    .await;

    let nz = source(&mock, "nz_points");
    assert_eq!(nz.tile_grid().id(), "NZTM2000Quad");
    assert_eq!(nz.get_tilejson().other["tileGrid"]["crs"], "EPSG:2193");
    let catalog = mock.0.tile_manager.tile_sources().get_catalog();
    assert_eq!(
        catalog["nz_points"].tile_grid.as_deref(),
        Some("NZTM2000Quad")
    );

    let points = source(&mock, "points1");
    assert!(points.tile_grid().is_web_mercator());
    assert!(!points.get_tilejson().other.contains_key("tileGrid"));
    assert_eq!(catalog["points1"].tile_grid, None);
}

#![cfg(feature = "postgres")]

use actix_http::Request;
use actix_web::http::StatusCode;
use actix_web::test::{call_and_read_body_json, call_service, read_body, TestRequest};
use ctor::ctor;
use indoc::indoc;
use insta::assert_yaml_snapshot;
use martin::OptOneMany;
use tilejson::TileJSON;

pub mod utils;
pub use utils::*;

#[ctor]
fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

macro_rules! create_app {
    ($sources:expr) => {{
        let cfg = mock_cfg(indoc::indoc!($sources));
        let state = mock_sources(cfg).await.0;
        ::actix_web::test::init_service(
            ::actix_web::App::new()
                .app_data(actix_web::web::Data::new(
                    ::martin::srv::Catalog::new(&state).unwrap(),
                ))
                .app_data(actix_web::web::Data::new(::martin::NO_MAIN_CACHE))
                .app_data(actix_web::web::Data::new(state.tiles))
                .configure(::martin::srv::router),
        )
        .await
    }};
}

fn test_get(path: &str) -> Request {
    TestRequest::get().uri(path).to_request()
}

#[actix_rt::test]
async fn pg_get_catalog() {
    let app = create_app! { "
postgres:
   connection_string: $DATABASE_URL
"};

    let req = test_get("/catalog");
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let body = read_body(response).await;
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_yaml_snapshot!(body, @r###"
    ---
    fonts: {}
    sprites: {}
    tiles:
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
}

#[actix_rt::test]
async fn pg_get_table_source_ok() {
    let app = create_app! { "
postgres:
  connection_string: $DATABASE_URL
  tables:
    bad_srid:
      schema: public
      table: table_source
      srid: 3857
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: GEOMETRY
      properties:
        gid: int4
    table_source:
      schema: public
      table: table_source
      srid: 4326
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: GEOMETRY
      properties:
        gid: int4
" };

    let req = test_get("/non_existent");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = test_get("/bad_srid");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn pg_get_table_source_rewrite() {
    let app = create_app! { "
postgres:
  connection_string: $DATABASE_URL
  tables:
    table_source:
      schema: public
      table: table_source
      srid: 4326
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: GEOMETRY
      properties:
        gid: int4
" };

    let req = TestRequest::get()
        .uri("/table_source?token=martin")
        .insert_header(("x-rewrite-url", "/tiles/table_source?token=martin"))
        .to_request();
    let result: TileJSON = call_and_read_body_json(&app, req).await;
    assert_yaml_snapshot!(result, @r###"
    ---
    tilejson: 3.0.0
    tiles:
      - "http://localhost:8080/tiles/table_source/{z}/{x}/{y}?token=martin"
    vector_layers:
      - id: table_source
        fields:
          gid: int4
    bounds:
      - -180
      - -90
      - 180
      - 90
    name: table_source
    foo:
      bar: foo
    "###);
}

#[actix_rt::test]
async fn pg_get_table_source_tile_ok() {
    let app = create_app! { "
postgres:
  connection_string: $DATABASE_URL
  tables:
    points2:
      schema: public
      table: points2
      srid: 4326
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    points1:
      schema: public
      table: points1
      srid: 4326
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    points_empty_srid:
      schema: public
      table: points_empty_srid
      srid: 900973
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: GEOMETRY
      properties:
        gid: int4
    table_source:
      schema: public
      table: table_source
      srid: 4326
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: GEOMETRY
      properties:
        gid: int4
    points3857:
      schema: public
      table: points3857
      srid: 3857
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    table_source_multiple_geom.geom1:
      schema: public
      table: table_source_multiple_geom
      srid: 4326
      geometry_column: geom1
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    table_source_multiple_geom.geom2:
      schema: public
      table: table_source_multiple_geom
      srid: 4326
      geometry_column: geom2
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    MIXPOINTS:
      schema: MIXEDCASE
      table: mixPoints
      srid: 4326
      geometry_column: geoM
      id_column: giD
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        tAble: text
" };

    let req = test_get("/non_existent/0/0/0");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = test_get("/table_source/0/0/0");
    let response = call_service(&app, req).await;
    assert_response(response).await;
}

#[actix_rt::test]
async fn pg_get_table_source_multiple_geom_tile_ok() {
    let app = create_app! { "
postgres:
  connection_string: $DATABASE_URL
  tables:
    points2:
      schema: public
      table: points2
      srid: 4326
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    table_source_multiple_geom.geom2:
      schema: public
      table: table_source_multiple_geom
      srid: 4326
      geometry_column: geom2
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    table_source:
      schema: public
      table: table_source
      srid: 4326
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: GEOMETRY
      properties:
        gid: int4
    points1:
      schema: public
      table: points1
      srid: 4326
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    MIXPOINTS:
      schema: MIXEDCASE
      table: mixPoints
      srid: 4326
      geometry_column: geoM
      id_column: giD
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        tAble: text
    points_empty_srid:
      schema: public
      table: points_empty_srid
      srid: 900973
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: GEOMETRY
      properties:
        gid: int4
    points3857:
      schema: public
      table: points3857
      srid: 3857
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    table_source_multiple_geom.geom1:
      schema: public
      table: table_source_multiple_geom
      srid: 4326
      geometry_column: geom1
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
"};

    let req = test_get("/table_source_multiple_geom.geom1/0/0/0");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    let req = test_get("/table_source_multiple_geom.geom2/0/0/0");
    let response = call_service(&app, req).await;
    assert_response(response).await;
}

#[actix_rt::test]
async fn pg_get_table_source_tile_minmax_zoom_ok() {
    let app = create_app! { "
postgres:
  connection_string: $DATABASE_URL
  tables:
    points3857:
      schema: public
      table: points3857
      srid: 3857
      geometry_column: geom
      minzoom: 6
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    points2:
      schema: public
      table: points2
      srid: 4326
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    points1:
      schema: public
      table: points1
      srid: 4326
      geometry_column: geom
      minzoom: 6
      maxzoom: 12
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    table_source:
      schema: public
      table: table_source
      srid: 4326
      geometry_column: geom
      maxzoom: 6
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: GEOMETRY
      properties:
        gid: int4
"};
    // zoom = 0 (nothing)
    let req = test_get("/points1/0/0/0");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // zoom = 6 (points1)
    let req = test_get("/points1/6/38/20");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    // zoom = 12 (points1)
    let req = test_get("/points1/12/2476/1280");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    // zoom = 13 (nothing)
    let req = test_get("/points1/13/4952/2560");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // zoom = 0 (points2)
    let req = test_get("/points2/0/0/0");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    // zoom = 6 (points2)
    let req = test_get("/points2/6/38/20");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    // zoom = 12 (points2)
    let req = test_get("/points2/12/2476/1280");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    // zoom = 13 (points2)
    let req = test_get("/points2/13/4952/2560");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    // zoom = 0 (nothing)
    let req = test_get("/points3857/0/0/0");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // zoom = 12 (points3857)
    let req = test_get("/points3857/12/2476/1280");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    // zoom = 0 (table_source)
    let req = test_get("/table_source/0/0/0");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    // zoom = 12 (nothing)
    let req = test_get("/table_source/12/2476/1280");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn pg_get_function_tiles() {
    let app = create_app! { "
postgres:
   connection_string: $DATABASE_URL
"};

    let req = test_get("/function_zoom_xy/6/38/20");
    assert!(call_service(&app, req).await.status().is_success());

    let req = test_get("/function_zxy/6/38/20");
    assert!(call_service(&app, req).await.status().is_success());

    let req = test_get("/function_zxy2/6/38/20");
    assert!(call_service(&app, req).await.status().is_success());

    let req = test_get("/function_zxy_query/6/38/20");
    assert!(call_service(&app, req).await.status().is_success());

    let req = test_get("/function_zxy_query_jsonb/6/38/20");
    assert!(call_service(&app, req).await.status().is_success());

    let req = test_get("/function_zxy_row/6/38/20");
    assert!(call_service(&app, req).await.status().is_success());

    let req = test_get("/function_Mixed_Name/6/38/20");
    assert!(call_service(&app, req).await.status().is_success());

    let req = test_get("/function_zxy_row_key/6/38/20");
    assert!(call_service(&app, req).await.status().is_success());
}

#[actix_rt::test]
async fn pg_get_composite_source_ok() {
    let app = create_app! { "
postgres:
  connection_string: $DATABASE_URL
  tables:
    table_source_multiple_geom.geom2:
      schema: public
      table: table_source_multiple_geom
      srid: 4326
      geometry_column: geom2
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    points2:
      schema: public
      table: points2
      srid: 4326
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    points_empty_srid:
      schema: public
      table: points_empty_srid
      srid: 900973
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: GEOMETRY
      properties:
        gid: int4
    table_source:
      schema: public
      table: table_source
      srid: 4326
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: GEOMETRY
      properties:
        gid: int4
    MIXPOINTS:
      schema: MIXEDCASE
      table: mixPoints
      srid: 4326
      geometry_column: geoM
      id_column: giD
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        tAble: text
    table_source_multiple_geom.geom1:
      schema: public
      table: table_source_multiple_geom
      srid: 4326
      geometry_column: geom1
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    points1:
      schema: public
      table: points1
      srid: 4326
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    points3857:
      schema: public
      table: points3857
      srid: 3857
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
"};
    let req = test_get("/non_existent1,non_existent2");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = test_get("/points1,points2,points3857");
    let response = call_service(&app, req).await;
    assert_response(response).await;
}

#[actix_rt::test]
async fn pg_get_composite_source_tile_ok() {
    let app = create_app! { "
postgres:
  connection_string: $DATABASE_URL
  tables:
    points_empty_srid:
      schema: public
      table: points_empty_srid
      srid: 900973
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: GEOMETRY
      properties:
        gid: int4
    table_source_multiple_geom.geom1:
      schema: public
      table: table_source_multiple_geom
      srid: 4326
      geometry_column: geom1
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    table_source_multiple_geom.geom2:
      schema: public
      table: table_source_multiple_geom
      srid: 4326
      geometry_column: geom2
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    table_source:
      schema: public
      table: table_source
      srid: 4326
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: GEOMETRY
      properties:
        gid: int4
    points1:
      schema: public
      table: points1
      srid: 4326
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    MIXPOINTS:
      schema: MIXEDCASE
      table: mixPoints
      srid: 4326
      geometry_column: geoM
      id_column: giD
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        tAble: text
    points2:
      schema: public
      table: points2
      srid: 4326
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    points3857:
      schema: public
      table: points3857
      srid: 3857
      geometry_column: geom
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
"};

    let req = test_get("/non_existent1,non_existent2/0/0/0");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = test_get("/points1,points2,points3857/0/0/0");
    let response = call_service(&app, req).await;
    assert_response(response).await;
}

#[actix_rt::test]
async fn pg_get_composite_source_tile_minmax_zoom_ok() {
    let app = create_app! { "
postgres:
  connection_string: $DATABASE_URL
  tables:
    points1:
      schema: public
      table: points1
      srid: 4326
      geometry_column: geom
      minzoom: 6
      maxzoom: 13
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
    points2:
      schema: public
      table: points2
      srid: 4326
      geometry_column: geom
      minzoom: 13
      maxzoom: 20
      bounds: [-180.0, -90.0, 180.0, 90.0]
      geometry_type: POINT
      properties:
        gid: int4
"};

    // zoom = 0 (nothing)
    let req = test_get("/points1,points2/0/0/0");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // zoom = 6 (points1)
    let req = test_get("/points1,points2/6/38/20");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    // zoom = 12 (points1)
    let req = test_get("/points1,points2/12/2476/1280");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    // zoom = 13 (points1, points2)
    let req = test_get("/points1,points2/13/4952/2560");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    // zoom = 14 (points2)
    let req = test_get("/points1,points2/14/9904/5121");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    // zoom = 20 (points2)
    let req = test_get("/points1,points2/20/633856/327787");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    // zoom = 21 (nothing)
    let req = test_get("/points1,points2/21/1267712/655574");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn pg_null_functions() {
    let app = create_app! { "
postgres:
   connection_string: $DATABASE_URL
"};

    let req = test_get("/function_null/0/0/0");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let req = test_get("/function_null_row/0/0/0");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let req = test_get("/function_null_row2/0/0/0");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[actix_rt::test]
async fn pg_get_function_source_ok() {
    let app = create_app! { "
postgres:
   connection_string: $DATABASE_URL
"};

    let req = test_get("/non_existent");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = test_get("/function_zoom_xy");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    let req = test_get("/function_zxy");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    let req = test_get("/function_zxy_query");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    let req = test_get("/function_zxy_query_jsonb");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    let req = test_get("/function_zxy_query_test");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    let req = test_get("/function_zxy_row");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    let req = test_get("/function_Mixed_Name");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    let req = test_get("/function_zxy_row_key");
    let response = call_service(&app, req).await;
    assert_response(response).await;
}

#[actix_rt::test]
async fn pg_get_function_source_ok_rewrite() {
    let app = create_app! { "
postgres:
  connection_string: $DATABASE_URL
"};

    let req = TestRequest::get()
        .uri("/function_zxy_query?token=martin")
        .insert_header(("x-rewrite-url", "/tiles/function_zxy_query?token=martin"))
        .to_request();
    let result: TileJSON = call_and_read_body_json(&app, req).await;
    assert_eq!(
        result.tiles,
        &["http://localhost:8080/tiles/function_zxy_query/{z}/{x}/{y}?token=martin"]
    );

    let req = TestRequest::get()
        .uri("/function_zxy_query_jsonb?token=martin")
        .insert_header((
            "x-rewrite-url",
            "/tiles/function_zxy_query_jsonb?token=martin",
        ))
        .to_request();
    let result: TileJSON = call_and_read_body_json(&app, req).await;
    assert_eq!(
        result.tiles,
        &["http://localhost:8080/tiles/function_zxy_query_jsonb/{z}/{x}/{y}?token=martin"]
    );
}

#[actix_rt::test]
async fn pg_get_function_source_tile_ok() {
    let app = create_app! { "
postgres:
  connection_string: $DATABASE_URL
"};

    let req = test_get("/function_zxy_query/0/0/0");
    let response = call_service(&app, req).await;
    assert_response(response).await;
}

#[actix_rt::test]
async fn pg_get_function_source_tile_minmax_zoom_ok() {
    let app = create_app! {"
postgres:
  connection_string: $DATABASE_URL
  functions:
    function_source1:
      schema: public
      function: function_zxy_query
    function_source2:
      schema: public
      function: function_zxy_query
      minzoom: 6
      maxzoom: 12
      bounds: [-180.0, -90.0, 180.0, 90.0]
"};

    // zoom = 0 (function_source1)
    let req = test_get("/function_source1/0/0/0");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    // zoom = 6 (function_source1)
    let req = test_get("/function_source1/6/38/20");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    // zoom = 12 (function_source1)
    let req = test_get("/function_source1/12/2476/1280");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    // zoom = 13 (function_source1)
    let req = test_get("/function_source1/13/4952/2560");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    // zoom = 0 (nothing)
    let req = test_get("/function_source2/0/0/0");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // zoom = 6 (function_source2)
    let req = test_get("/function_source2/6/38/20");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    // zoom = 12 (function_source2)
    let req = test_get("/function_source2/12/2476/1280");
    let response = call_service(&app, req).await;
    assert_response(response).await;

    // zoom = 13 (nothing)
    let req = test_get("/function_source2/13/4952/2560");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn pg_get_function_source_query_params_ok() {
    let app = create_app! { "
postgres:
  connection_string: $DATABASE_URL
"};

    let req = test_get("/function_zxy_query_test/0/0/0");
    let response = call_service(&app, req).await;
    assert!(response.status().is_server_error());

    let req = test_get("/function_zxy_query_test/0/0/0?token=martin");
    let response = call_service(&app, req).await;
    assert_response(response).await;
}

#[actix_rt::test]
async fn pg_get_health_returns_ok() {
    let app = create_app! { "
postgres:
  connection_string: $DATABASE_URL
"};

    let req = test_get("/health");
    let response = call_service(&app, req).await;
    assert_response(response).await;
}

#[actix_rt::test]
async fn pg_tables_feature_id() {
    let cfg = mock_pgcfg(indoc! {"
connection_string: $DATABASE_URL
tables:
  id_and_prop:
    schema: MIXEDCASE
    table: mixPoints
    srid: 4326
    geometry_column: geoM
    id_column: giD
    bounds: [-180.0, -90.0, 180.0, 90.0]
    geometry_type: POINT
    properties:
      TABLE: text
      giD: int4
  no_id:
    schema: MIXEDCASE
    table: mixPoints
    srid: 4326
    geometry_column: geoM
    bounds: [-180.0, -90.0, 180.0, 90.0]
    geometry_type: POINT
    properties:
      TABLE: text
  id_only:
    schema: MIXEDCASE
    table: mixPoints
    srid: 4326
    geometry_column: geoM
    id_column: giD
    bounds: [-180.0, -90.0, 180.0, 90.0]
    geometry_type: POINT
    properties:
      TABLE: text
  prop_only:
    schema: MIXEDCASE
    table: mixPoints
    srid: 4326
    geometry_column: geoM
    bounds: [-180.0, -90.0, 180.0, 90.0]
    geometry_type: POINT
    properties:
      giD: int4
      TABLE: text
"});
    let mock = mock_sources(cfg.clone()).await;

    let src = table(&mock, "no_id");
    assert_eq!(src.id_column, None);
    assert!(matches!(&src.properties, Some(v) if v.len() == 1));
    let tj = source(&mock, "no_id").get_tilejson();
    assert_yaml_snapshot!(tj, @r###"
    ---
    tilejson: 3.0.0
    tiles: []
    vector_layers:
      - id: MixPoints
        fields:
          Gid: int4
          TABLE: text
    bounds:
      - -180
      - -90
      - 180
      - 90
    description: a description from comment on table
    name: no_id
    "###);

    assert_yaml_snapshot!(table(&mock, "id_only"), @r###"
    ---
    schema: MixedCase
    table: MixPoints
    srid: 4326
    geometry_column: Geom
    id_column: giD
    bounds:
      - -180
      - -90
      - 180
      - 90
    geometry_type: POINT
    properties:
      TABLE: text
    "###);

    assert_yaml_snapshot!(table(&mock, "id_and_prop"), @r###"
    ---
    schema: MixedCase
    table: MixPoints
    srid: 4326
    geometry_column: Geom
    id_column: giD
    bounds:
      - -180
      - -90
      - 180
      - 90
    geometry_type: POINT
    properties:
      TABLE: text
      giD: int4
    "###);

    assert_yaml_snapshot!(table(&mock, "prop_only"), @r###"
    ---
    schema: MixedCase
    table: MixPoints
    srid: 4326
    geometry_column: Geom
    bounds:
      - -180
      - -90
      - 180
      - 90
    geometry_type: POINT
    properties:
      TABLE: text
      giD: int4
    "###);

    // --------------------------------------------

    let state = mock_sources(cfg.clone()).await.0;
    let app = ::actix_web::test::init_service(
        ::actix_web::App::new()
            .app_data(actix_web::web::Data::new(
                ::martin::srv::Catalog::new(&state).unwrap(),
            ))
            .app_data(actix_web::web::Data::new(::martin::NO_MAIN_CACHE))
            .app_data(actix_web::web::Data::new(state.tiles))
            .configure(::martin::srv::router),
    )
    .await;

    let OptOneMany::One(cfg) = cfg.postgres else {
        panic!()
    };
    for (name, _) in cfg.tables.unwrap_or_default() {
        let req = test_get(format!("/{name}/0/0/0").as_str());
        let response = call_service(&app, req).await;
        assert_response(response).await;
    }
}

use actix_http::Request;
use actix_web::http::StatusCode;
use actix_web::test::{call_and_read_body_json, call_service, read_body, TestRequest};
use ctor::ctor;
use martin::pg::config::{FunctionInfo, TableInfo};
use martin::srv::server::IndexEntry;
use std::collections::HashMap;
use tilejson::{Bounds, TileJSON};

#[path = "utils.rs"]
mod utils;
use utils::*;

#[ctor]
fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

macro_rules! create_app {
    ($sources:expr) => {{
        let state = crate::utils::mock_app_data($sources.await).await;
        ::actix_web::test::init_service(
            ::actix_web::App::new()
                .app_data(state)
                .configure(::martin::srv::server::router),
        )
        .await
    }};
}

fn test_get(path: &str) -> Request {
    TestRequest::get().uri(path).to_request()
}

#[actix_rt::test]
async fn get_table_catalog_ok() {
    let app = create_app!(mock_default_table_sources());

    let req = test_get("/catalog");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    let body = read_body(response).await;
    let sources: Vec<IndexEntry> = serde_json::from_slice(&body).unwrap();
    let expected = "table_source";
    assert_eq!(sources.into_iter().filter(|v| v.id == expected).count(), 1);
}

#[actix_rt::test]
async fn get_function_catalog_ok() {
    let app = create_app!(mock_default_function_sources());

    let req = test_get("/catalog");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    let body = read_body(response).await;
    let sources: Vec<IndexEntry> = serde_json::from_slice(&body).unwrap();
    let expected = "function_zxy_query";
    assert_eq!(sources.into_iter().filter(|v| v.id == expected).count(), 1);
}

#[actix_rt::test]
async fn get_table_source_ok() {
    let table_source = TableInfo {
        schema: "public".to_owned(),
        table: "table_source".to_owned(),
        id_column: None,
        geometry_column: "geom".to_owned(),
        bounds: Some(Bounds::MAX),
        minzoom: Some(0),
        maxzoom: Some(30),
        srid: 4326,
        extent: Some(4096),
        buffer: Some(64),
        clip_geom: Some(true),
        geometry_type: None,
        properties: HashMap::new(),
        unrecognized: HashMap::new(),
    };

    let app = create_app!(mock_sources(None, Some(&[("table_source", table_source)])));

    let req = test_get("/non_existent");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = TestRequest::get()
        .uri("/table_source?token=martin")
        .insert_header(("x-rewrite-url", "/tiles/table_source?token=martin"))
        .to_request();
    let result: TileJSON = call_and_read_body_json(&app, req).await;
    assert_eq!(result.name, Some(String::from("public.table_source.geom")));
    assert_eq!(
        result.tiles,
        &["http://localhost:8080/tiles/table_source/{z}/{x}/{y}?token=martin"]
    );
    assert_eq!(result.minzoom, Some(0));
    assert_eq!(result.maxzoom, Some(30));
    assert_eq!(result.bounds, Some(Bounds::MAX));
}

#[actix_rt::test]
async fn get_table_source_tile_ok() {
    let app = create_app!(mock_default_table_sources());

    let req = test_get("/non_existent/0/0/0");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = test_get("/table_source/0/0/0");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn get_table_source_multiple_geom_tile_ok() {
    let app = create_app!(mock_default_table_sources());

    let req = test_get("/table_source_multiple_geom.geom1/0/0/0");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    let req = test_get("/table_source_multiple_geom.geom2/0/0/0");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn get_table_source_tile_minmax_zoom_ok() {
    let table_source = TableInfo {
        schema: "public".to_owned(),
        table: "table_source".to_owned(),
        id_column: None,
        geometry_column: "geom".to_owned(),
        bounds: Some(Bounds::MAX),
        minzoom: None,
        maxzoom: Some(6),
        srid: 4326,
        extent: Some(4096),
        buffer: Some(64),
        clip_geom: Some(true),
        geometry_type: None,
        properties: HashMap::new(),
        unrecognized: HashMap::new(),
    };

    let points1 = TableInfo {
        schema: "public".to_owned(),
        table: "points1".to_owned(),
        id_column: None,
        geometry_column: "geom".to_owned(),
        minzoom: Some(6),
        maxzoom: Some(12),
        geometry_type: None,
        properties: HashMap::new(),
        unrecognized: HashMap::new(),
        ..table_source
    };

    let points2 = TableInfo {
        schema: "public".to_owned(),
        table: "points2".to_owned(),
        id_column: None,
        geometry_column: "geom".to_owned(),
        minzoom: None,
        maxzoom: None,
        geometry_type: None,
        properties: HashMap::new(),
        unrecognized: HashMap::new(),
        ..table_source
    };

    let points3857 = TableInfo {
        schema: "public".to_owned(),
        table: "points3857".to_owned(),
        id_column: None,
        geometry_column: "geom".to_owned(),
        minzoom: Some(6),
        maxzoom: None,
        geometry_type: None,
        properties: HashMap::new(),
        unrecognized: HashMap::new(),
        ..table_source
    };

    let tables = &[
        ("points1", points1),
        ("points2", points2),
        ("points3857", points3857),
        ("table_source", table_source),
    ];
    let app = create_app!(mock_sources(None, Some(tables)));

    // zoom = 0 (nothing)
    let req = test_get("/points1/0/0/0");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // zoom = 6 (points1)
    let req = test_get("/points1/6/38/20");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 12 (points1)
    let req = test_get("/points1/12/2476/1280");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 13 (nothing)
    let req = test_get("/points1/13/4952/2560");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // zoom = 0 (points2)
    let req = test_get("/points2/0/0/0");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 6 (points2)
    let req = test_get("/points2/6/38/20");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 12 (points2)
    let req = test_get("/points2/12/2476/1280");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 13 (points2)
    let req = test_get("/points2/13/4952/2560");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 0 (nothing)
    let req = test_get("/points3857/0/0/0");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // zoom = 12 (points3857)
    let req = test_get("/points3857/12/2476/1280");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 0 (table_source)
    let req = test_get("/table_source/0/0/0");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 12 (nothing)
    let req = test_get("/table_source/12/2476/1280");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn get_composite_source_ok() {
    let app = create_app!(mock_default_table_sources());

    let req = test_get("/non_existent1,non_existent2");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = test_get("/points1,points2,points3857");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn get_composite_source_tile_ok() {
    let app = create_app!(mock_default_table_sources());

    let req = test_get("/non_existent1,non_existent2/0/0/0");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = test_get("/points1,points2,points3857/0/0/0");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn get_composite_source_tile_minmax_zoom_ok() {
    let public_points1 = TableInfo {
        schema: "public".to_owned(),
        table: "points1".to_owned(),
        id_column: None,
        geometry_column: "geom".to_owned(),
        bounds: Some(Bounds::MAX),
        minzoom: Some(6),
        maxzoom: Some(13),
        srid: 4326,
        extent: Some(4096),
        buffer: Some(64),
        clip_geom: Some(true),
        geometry_type: None,
        properties: HashMap::new(),
        unrecognized: HashMap::new(),
    };

    let public_points2 = TableInfo {
        schema: "public".to_owned(),
        table: "points2".to_owned(),
        id_column: None,
        geometry_column: "geom".to_owned(),
        bounds: Some(Bounds::MAX),
        minzoom: Some(13),
        maxzoom: Some(20),
        srid: 4326,
        extent: Some(4096),
        buffer: Some(64),
        clip_geom: Some(true),
        geometry_type: None,
        properties: HashMap::new(),
        unrecognized: HashMap::new(),
    };

    let tables = &[("points1", public_points1), ("points2", public_points2)];
    let app = create_app!(mock_sources(None, Some(tables)));

    // zoom = 0 (nothing)
    let req = test_get("/points1,points2/0/0/0");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // zoom = 6 (points1)
    let req = test_get("/points1,points2/6/38/20");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 12 (points1)
    let req = test_get("/points1,points2/12/2476/1280");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 13 (points1, points2)
    let req = test_get("/points1,points2/13/4952/2560");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 14 (points2)
    let req = test_get("/points1,points2/14/9904/5121");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 20 (points2)
    let req = test_get("/points1,points2/20/633856/327787");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 21 (nothing)
    let req = test_get("/points1,points2/21/1267712/655574");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn get_function_source_ok() {
    let app = create_app!(mock_default_function_sources());

    let req = test_get("/non_existent");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = test_get("/function_zxy_query");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    let req = TestRequest::get()
        .uri("/function_zxy_query?token=martin")
        .insert_header(("x-rewrite-url", "/tiles/function_zxy_query?token=martin"))
        .to_request();
    let result: TileJSON = call_and_read_body_json(&app, req).await;
    assert_eq!(
        result.tiles,
        &["http://localhost:8080/tiles/function_zxy_query/{z}/{x}/{y}?token=martin"]
    );
}

#[actix_rt::test]
async fn get_function_source_tile_ok() {
    let app = create_app!(mock_default_function_sources());

    let req = test_get("/function_zxy_query/0/0/0");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn get_function_source_tile_minmax_zoom_ok() {
    let function_source1 = FunctionInfo {
        schema: "public".to_owned(),
        function: "function_zxy_query".to_owned(),
        minzoom: None,
        maxzoom: None,
        bounds: Some(Bounds::MAX),
        unrecognized: HashMap::new(),
    };

    let function_source2 = FunctionInfo {
        schema: "public".to_owned(),
        function: "function_zxy_query".to_owned(),
        minzoom: Some(6),
        maxzoom: Some(12),
        bounds: Some(Bounds::MAX),
        unrecognized: HashMap::new(),
    };

    let funcs = &[
        ("function_source1", function_source1),
        ("function_source2", function_source2),
    ];
    let app = create_app!(mock_sources(Some(funcs), None));

    // zoom = 0 (function_source1)
    let req = test_get("/function_source1/0/0/0");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 6 (function_source1)
    let req = test_get("/function_source1/6/38/20");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 12 (function_source1)
    let req = test_get("/function_source1/12/2476/1280");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 13 (function_source1)
    let req = test_get("/function_source1/13/4952/2560");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 0 (nothing)
    let req = test_get("/function_source2/0/0/0");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // zoom = 6 (function_source2)
    let req = test_get("/function_source2/6/38/20");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 12 (function_source2)
    let req = test_get("/function_source2/12/2476/1280");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 13 (nothing)
    let req = test_get("/function_source2/13/4952/2560");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn get_function_source_query_params_ok() {
    let app = create_app!(mock_default_function_sources());

    let req = test_get("/function_zxy_query_test/0/0/0");
    let response = call_service(&app, req).await;
    println!("response.status = {:?}", response.status());
    assert!(response.status().is_server_error());

    let req = test_get("/function_zxy_query_test/0/0/0?token=martin");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn get_health_returns_ok() {
    let app = create_app!(mock_default_function_sources());

    let req = test_get("/health");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

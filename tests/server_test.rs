use actix_http::Request;
use actix_web::http::StatusCode;
use actix_web::test::{call_and_read_body_json, call_service, read_body, TestRequest};
use martin::pg::dev::{
    mock_default_function_sources, mock_default_table_sources, mock_function_sources, mock_state,
    mock_table_sources,
};
use martin::pg::function_source::{FunctionSource, FunctionSources};
use martin::pg::table_source::{TableSource, TableSources};
use std::collections::HashMap;
use tilejson::{Bounds, TileJSON};

fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

macro_rules! create_app {
    ($tables:expr, $functions:expr) => {{
        init();
        let state = mock_state($tables, $functions).await;
        let data = ::actix_web::web::Data::new(state);
        ::actix_web::test::init_service(
            ::actix_web::App::new()
                .app_data(data)
                .configure(::martin::srv::server::router),
        )
        .await
    }};
}

fn test_get(path: &str) -> Request {
    TestRequest::get().uri(path).to_request()
}

#[actix_rt::test]
async fn get_table_sources_ok() {
    let app = create_app!(Some(mock_default_table_sources()), None);

    let req = test_get("/index.json");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    let body = read_body(response).await;
    let table_sources: TableSources = serde_json::from_slice(&body).unwrap();
    assert!(table_sources.contains_key("public.table_source"));
}

#[actix_rt::test]
async fn get_table_sources_watch_mode_ok() {
    let app = create_app!(Some(mock_default_table_sources()), None);

    let req = test_get("/index.json");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    let body = read_body(response).await;
    let table_sources: TableSources = serde_json::from_slice(&body).unwrap();
    assert!(table_sources.contains_key("public.table_source"));
}

#[actix_rt::test]
async fn get_table_source_ok() {
    let table_source = TableSource {
        id: "public.table_source".to_owned(),
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

    let app = create_app!(Some(mock_table_sources(&[table_source])), None);

    let req = test_get("/public.non_existent.json");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = TestRequest::get()
        .uri("/public.table_source.json?token=martin")
        .insert_header((
            "x-rewrite-url",
            "/tiles/public.table_source.json?token=martin",
        ))
        .to_request();
    let result: TileJSON = call_and_read_body_json(&app, req).await;
    assert_eq!(result.name, Some(String::from("public.table_source")));
    assert_eq!(
        result.tiles,
        vec!["http://localhost:8080/tiles/public.table_source/{z}/{x}/{y}.pbf?token=martin"]
    );
    assert_eq!(result.minzoom, Some(0));
    assert_eq!(result.maxzoom, Some(30));
    assert_eq!(result.bounds, Some(Bounds::MAX));
}

#[actix_rt::test]
async fn get_table_source_tile_ok() {
    let app = create_app!(Some(mock_default_table_sources()), None);

    let req = test_get("/public.non_existent/0/0/0.pbf");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = test_get("/public.table_source/0/0/0.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn get_table_source_multiple_geom_tile_ok() {
    let app = create_app!(Some(mock_default_table_sources()), None);

    let req = test_get("/public.table_source_multiple_geom.geom1/0/0/0.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    let req = test_get("/public.table_source_multiple_geom.geom2/0/0/0.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn get_table_source_tile_minmax_zoom_ok() {
    init();

    let table_source = TableSource {
        id: "public.table_source".to_owned(),
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

    let points1 = TableSource {
        id: "public.points1".to_owned(),
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

    let points2 = TableSource {
        id: "public.points2".to_owned(),
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

    let points3857 = TableSource {
        id: "public.points3857".to_owned(),
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

    let tables = &[points1, points2, points3857, table_source];
    let app = create_app!(Some(mock_table_sources(tables)), None);

    // zoom = 0 (nothing)
    let req = test_get("/public.points1/0/0/0.pbf");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // zoom = 6 (public.points1)
    let req = test_get("/public.points1/6/38/20.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 12 (public.points1)
    let req = test_get("/public.points1/12/2476/1280.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 13 (nothing)
    let req = test_get("/public.points1/13/4952/2560.pbf");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // zoom = 0 (public.points2)
    let req = test_get("/public.points2/0/0/0.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 6 (public.points2)
    let req = test_get("/public.points2/6/38/20.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 12 (public.points2)
    let req = test_get("/public.points2/12/2476/1280.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 13 (public.points2)
    let req = test_get("/public.points2/13/4952/2560.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 0 (nothing)
    let req = test_get("/public.points3857/0/0/0.pbf");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // zoom = 12 (public.points3857)
    let req = test_get("/public.points3857/12/2476/1280.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 0 (public.table_source)
    let req = test_get("/public.table_source/0/0/0.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 12 (nothing)
    let req = test_get("/public.table_source/12/2476/1280.pbf");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn get_composite_source_ok() {
    let app = create_app!(Some(mock_default_table_sources()), None);

    let req = test_get("/public.non_existent1,public.non_existent2.json");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = test_get("/public.points1,public.points2,public.points3857.json");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn get_composite_source_tile_ok() {
    let app = create_app!(Some(mock_default_table_sources()), None);

    let req = test_get("/public.non_existent1,public.non_existent2/0/0/0.pbf");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = test_get("/public.points1,public.points2,public.points3857/0/0/0.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn get_composite_source_tile_minmax_zoom_ok() {
    init();

    let public_points1 = TableSource {
        id: "public.points1".to_owned(),
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

    let public_points2 = TableSource {
        id: "public.points2".to_owned(),
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

    let tables = &[public_points1, public_points2];
    let app = create_app!(Some(mock_table_sources(tables)), None);

    // zoom = 0 (nothing)
    let req = test_get("/public.points1,public.points2/0/0/0.pbf");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // zoom = 6 (public.points1)
    let req = test_get("/public.points1,public.points2/6/38/20.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 12 (public.points1)
    let req = test_get("/public.points1,public.points2/12/2476/1280.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 13 (public.points1, public.points2)
    let req = test_get("/public.points1,public.points2/13/4952/2560.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 14 (public.points2)
    let req = test_get("/public.points1,public.points2/14/9904/5121.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 20 (public.points2)
    let req = test_get("/public.points1,public.points2/20/633856/327787.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 21 (nothing)
    let req = test_get("/public.points1,public.points2/21/1267712/655574.pbf");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn get_function_sources_ok() {
    let app = create_app!(None, Some(mock_default_function_sources()));

    let req = test_get("/rpc/index.json");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    let body = read_body(response).await;
    let function_sources: FunctionSources = serde_json::from_slice(&body).unwrap();
    assert!(function_sources.contains_key("public.function_source"));
}

#[actix_rt::test]
async fn get_function_sources_watch_mode_ok() {
    let app = create_app!(None, Some(mock_default_function_sources()));

    let req = test_get("/rpc/index.json");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    let body = read_body(response).await;
    let function_sources: FunctionSources = serde_json::from_slice(&body).unwrap();
    assert!(function_sources.contains_key("public.function_source"));
}

#[actix_rt::test]
async fn get_function_source_ok() {
    let app = create_app!(None, Some(mock_default_function_sources()));

    let req = test_get("/rpc/public.non_existent.json");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = test_get("/rpc/public.function_source.json");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    let req = TestRequest::get()
        .uri("/rpc/public.function_source.json?token=martin")
        .insert_header((
            "x-rewrite-url",
            "/tiles/rpc/public.function_source.json?token=martin",
        ))
        .to_request();
    let result: TileJSON = call_and_read_body_json(&app, req).await;
    assert_eq!(
        result.tiles,
        vec!["http://localhost:8080/tiles/rpc/public.function_source/{z}/{x}/{y}.pbf?token=martin"]
    );
}

#[actix_rt::test]
async fn get_function_source_tile_ok() {
    let app = create_app!(None, Some(mock_default_function_sources()));

    let req = test_get("/rpc/public.function_source/0/0/0.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn get_function_source_tile_minmax_zoom_ok() {
    let function_source1 = FunctionSource {
        id: "public.function_source1".to_owned(),
        schema: "public".to_owned(),
        function: "function_source".to_owned(),
        minzoom: None,
        maxzoom: None,
        bounds: Some(Bounds::MAX),
        unrecognized: HashMap::new(),
    };

    let function_source2 = FunctionSource {
        id: "public.function_source2".to_owned(),
        schema: "public".to_owned(),
        function: "function_source".to_owned(),
        minzoom: Some(6),
        maxzoom: Some(12),
        bounds: Some(Bounds::MAX),
        unrecognized: HashMap::new(),
    };

    let funcs = &[function_source1, function_source2];
    let app = create_app!(None, Some(mock_function_sources(funcs)));

    // zoom = 0 (public.function_source1)
    let req = test_get("/rpc/public.function_source1/0/0/0.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 6 (public.function_source1)
    let req = test_get("/rpc/public.function_source1/6/38/20.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 12 (public.function_source1)
    let req = test_get("/rpc/public.function_source1/12/2476/1280.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 13 (public.function_source1)
    let req = test_get("/rpc/public.function_source1/13/4952/2560.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 0 (nothing)
    let req = test_get("/rpc/public.function_source2/0/0/0.pbf");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // zoom = 6 (public.function_source2)
    let req = test_get("/rpc/public.function_source2/6/38/20.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 12 (public.function_source2)
    let req = test_get("/rpc/public.function_source2/12/2476/1280.pbf");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 13 (nothing)
    let req = test_get("/rpc/public.function_source2/13/4952/2560.pbf");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn null_function_204() {
    let null_function = FunctionSource {
        id: "public.null_function".to_owned(),
        schema: "public".to_owned(),
        function: "null_function".to_owned(),
        minzoom: None,
        maxzoom: None,
        bounds: None,
        unrecognized: HashMap::new(),
    };
    let app = create_app!(None, Some(mock_function_sources(&[null_function])));

    let req = test_get("/rpc/public.null_function/0/0/0.pbf");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[actix_rt::test]
async fn get_function_source_query_params_ok() {
    let app = create_app!(None, Some(mock_default_function_sources()));

    let req = test_get("/rpc/public.function_source_query_params/0/0/0.pbf");
    let response = call_service(&app, req).await;
    println!("response.status = {:?}", response.status());
    assert!(response.status().is_server_error());

    let req = test_get("/rpc/public.function_source_query_params/0/0/0.pbf?token=martin");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn get_health_returns_ok() {
    let app = create_app!(None, Some(mock_default_function_sources()));

    let req = test_get("/healthz");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

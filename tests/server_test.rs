use std::collections::HashMap;

use actix_web::web::Data;
use actix_web::{http, test, App};
use martin::pg::dev;
use martin::pg::function_source::{FunctionSource, FunctionSources};
use martin::pg::table_source::{TableSource, TableSources};
use martin::srv::server::router;
use tilejson::{Bounds, TileJSON};

fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[actix_rt::test]
async fn test_get_table_sources_ok() {
    init();

    let state = dev::mock_state(Some(dev::mock_default_table_sources()), None, None).await;
    let app = test::init_service(App::new().app_data(Data::new(state)).configure(router)).await;

    let req = test::TestRequest::get().uri("/index.json").to_request();
    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    let body = test::read_body(response).await;
    let table_sources: TableSources = serde_json::from_slice(&body).unwrap();
    assert!(table_sources.contains_key("public.table_source"));
}

#[actix_rt::test]
async fn test_get_table_sources_watch_mode_ok() {
    init();

    let state = dev::mock_state(Some(dev::mock_default_table_sources()), None, None).await;
    let app = test::init_service(App::new().app_data(Data::new(state)).configure(router)).await;

    let req = test::TestRequest::get().uri("/index.json").to_request();
    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    let body = test::read_body(response).await;
    let table_sources: TableSources = serde_json::from_slice(&body).unwrap();
    assert!(table_sources.contains_key("public.table_source"));
}

#[actix_rt::test]
async fn test_get_table_source_ok() {
    init();

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
    };

    let state = dev::mock_state(
        Some(dev::mock_table_sources(vec![table_source])),
        None,
        None,
    )
    .await;
    let app = test::init_service(App::new().app_data(Data::new(state)).configure(router)).await;

    let req = test::TestRequest::get()
        .uri("/public.non_existant.json")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert_eq!(response.status(), http::StatusCode::NOT_FOUND);

    let req = test::TestRequest::get()
        .uri("/public.table_source.json?token=martin")
        .insert_header((
            "x-rewrite-url",
            "/tiles/public.table_source.json?token=martin",
        ))
        .to_request();

    let result: TileJSON = test::call_and_read_body_json(&app, req).await;

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
async fn test_get_table_source_tile_ok() {
    init();

    let state = dev::mock_state(Some(dev::mock_default_table_sources()), None, None).await;
    let app = test::init_service(App::new().app_data(Data::new(state)).configure(router)).await;

    let req = test::TestRequest::get()
        .uri("/public.non_existant/0/0/0.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert_eq!(response.status(), http::StatusCode::NOT_FOUND);

    let req = test::TestRequest::get()
        .uri("/public.table_source/0/0/0.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn test_get_table_source_multiple_geom_tile_ok() {
    init();

    let state = dev::mock_state(Some(dev::mock_default_table_sources()), None, None).await;
    let app = test::init_service(App::new().app_data(Data::new(state)).configure(router)).await;

    let req = test::TestRequest::get()
        .uri("/public.table_source_multiple_geom.geom1/0/0/0.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    let req = test::TestRequest::get()
        .uri("/public.table_source_multiple_geom.geom2/0/0/0.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn test_get_table_source_tile_minmax_zoom_ok() {
    init();

    let points1 = TableSource {
        id: "public.points1".to_owned(),
        schema: "public".to_owned(),
        table: "points1".to_owned(),
        id_column: None,
        geometry_column: "geom".to_owned(),
        bounds: Some(Bounds::MAX),
        minzoom: Some(6),
        maxzoom: Some(12),
        srid: 4326,
        extent: Some(4096),
        buffer: Some(64),
        clip_geom: Some(true),
        geometry_type: None,
        properties: HashMap::new(),
    };

    let points2 = TableSource {
        id: "public.points2".to_owned(),
        schema: "public".to_owned(),
        table: "points2".to_owned(),
        id_column: None,
        geometry_column: "geom".to_owned(),
        bounds: Some(Bounds::MAX),
        minzoom: None,
        maxzoom: None,
        srid: 4326,
        extent: Some(4096),
        buffer: Some(64),
        clip_geom: Some(true),
        geometry_type: None,
        properties: HashMap::new(),
    };

    let points3857 = TableSource {
        id: "public.points3857".to_owned(),
        schema: "public".to_owned(),
        table: "points3857".to_owned(),
        id_column: None,
        geometry_column: "geom".to_owned(),
        bounds: Some(Bounds::MAX),
        minzoom: Some(6),
        maxzoom: None,
        srid: 4326,
        extent: Some(4096),
        buffer: Some(64),
        clip_geom: Some(true),
        geometry_type: None,
        properties: HashMap::new(),
    };

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
    };

    let state = dev::mock_state(
        Some(dev::mock_table_sources(vec![
            points1,
            points2,
            points3857,
            table_source,
        ])),
        None,
        None,
    )
    .await;

    let app = test::init_service(App::new().app_data(Data::new(state)).configure(router)).await;

    // zoom = 0 (nothing)
    let req = test::TestRequest::get()
        .uri("/public.points1/0/0/0.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert_eq!(response.status(), http::StatusCode::NOT_FOUND);

    // zoom = 6 (public.points1)
    let req = test::TestRequest::get()
        .uri("/public.points1/6/38/20.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 12 (public.points1)
    let req = test::TestRequest::get()
        .uri("/public.points1/12/2476/1280.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 13 (nothing)
    let req = test::TestRequest::get()
        .uri("/public.points1/13/4952/2560.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert_eq!(response.status(), http::StatusCode::NOT_FOUND);

    // zoom = 0 (public.points2)
    let req = test::TestRequest::get()
        .uri("/public.points2/0/0/0.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 6 (public.points2)
    let req = test::TestRequest::get()
        .uri("/public.points2/6/38/20.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 12 (public.points2)
    let req = test::TestRequest::get()
        .uri("/public.points2/12/2476/1280.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 13 (public.points2)
    let req = test::TestRequest::get()
        .uri("/public.points2/13/4952/2560.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 0 (nothing)
    let req = test::TestRequest::get()
        .uri("/public.points3857/0/0/0.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert_eq!(response.status(), http::StatusCode::NOT_FOUND);

    // zoom = 12 (public.points3857)
    let req = test::TestRequest::get()
        .uri("/public.points3857/12/2476/1280.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 0 (public.table_source)
    let req = test::TestRequest::get()
        .uri("/public.table_source/0/0/0.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 12 (nothing)
    let req = test::TestRequest::get()
        .uri("/public.table_source/12/2476/1280.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert_eq!(response.status(), http::StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn test_get_composite_source_ok() {
    init();

    let state = dev::mock_state(Some(dev::mock_default_table_sources()), None, None).await;
    let app = test::init_service(App::new().app_data(Data::new(state)).configure(router)).await;

    let req = test::TestRequest::get()
        .uri("/public.non_existant1,public.non_existant2.json")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert_eq!(response.status(), http::StatusCode::NOT_FOUND);

    let req = test::TestRequest::get()
        .uri("/public.points1,public.points2,public.points3857.json")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn test_get_composite_source_tile_ok() {
    init();

    let state = dev::mock_state(Some(dev::mock_default_table_sources()), None, None).await;
    let app = test::init_service(App::new().app_data(Data::new(state)).configure(router)).await;

    let req = test::TestRequest::get()
        .uri("/public.non_existant1,public.non_existant2/0/0/0.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert_eq!(response.status(), http::StatusCode::NOT_FOUND);

    let req = test::TestRequest::get()
        .uri("/public.points1,public.points2,public.points3857/0/0/0.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn test_get_composite_source_tile_minmax_zoom_ok() {
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
    };

    let state = dev::mock_state(
        Some(dev::mock_table_sources(vec![
            public_points1,
            public_points2,
        ])),
        None,
        None,
    )
    .await;
    let app = test::init_service(App::new().app_data(Data::new(state)).configure(router)).await;

    // zoom = 0 (nothing)
    let req = test::TestRequest::get()
        .uri("/public.points1,public.points2/0/0/0.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert_eq!(response.status(), http::StatusCode::NOT_FOUND);

    // zoom = 6 (public.points1)
    let req = test::TestRequest::get()
        .uri("/public.points1,public.points2/6/38/20.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 12 (public.points1)
    let req = test::TestRequest::get()
        .uri("/public.points1,public.points2/12/2476/1280.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 13 (public.points1, public.points2)
    let req = test::TestRequest::get()
        .uri("/public.points1,public.points2/13/4952/2560.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 14 (public.points2)
    let req = test::TestRequest::get()
        .uri("/public.points1,public.points2/14/9904/5121.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 20 (public.points2)
    let req = test::TestRequest::get()
        .uri("/public.points1,public.points2/20/633856/327787.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 21 (nothing)
    let req = test::TestRequest::get()
        .uri("/public.points1,public.points2/21/1267712/655574.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert_eq!(response.status(), http::StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn test_get_function_sources_ok() {
    init();

    let state = dev::mock_state(None, Some(dev::mock_default_function_sources()), None).await;
    let app = test::init_service(App::new().app_data(Data::new(state)).configure(router)).await;

    let req = test::TestRequest::get().uri("/rpc/index.json").to_request();
    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    let body = test::read_body(response).await;
    let function_sources: FunctionSources = serde_json::from_slice(&body).unwrap();
    assert!(function_sources.contains_key("public.function_source"));
}

#[actix_rt::test]
async fn test_get_function_sources_watch_mode_ok() {
    init();

    let state = dev::mock_state(None, Some(dev::mock_default_function_sources()), None).await;
    let app = test::init_service(App::new().app_data(Data::new(state)).configure(router)).await;

    let req = test::TestRequest::get().uri("/rpc/index.json").to_request();
    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    let body = test::read_body(response).await;
    let function_sources: FunctionSources = serde_json::from_slice(&body).unwrap();
    assert!(function_sources.contains_key("public.function_source"));
}

#[actix_rt::test]
async fn test_get_function_source_ok() {
    init();

    let state = dev::mock_state(None, Some(dev::mock_default_function_sources()), None).await;
    let app = test::init_service(App::new().app_data(Data::new(state)).configure(router)).await;

    let req = test::TestRequest::get()
        .uri("/rpc/public.non_existant.json")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert_eq!(response.status(), http::StatusCode::NOT_FOUND);

    let req = test::TestRequest::get()
        .uri("/rpc/public.function_source.json")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    let req = test::TestRequest::get()
        .uri("/rpc/public.function_source.json?token=martin")
        .insert_header((
            "x-rewrite-url",
            "/tiles/rpc/public.function_source.json?token=martin",
        ))
        .to_request();

    let result: TileJSON = test::call_and_read_body_json(&app, req).await;

    assert_eq!(
        result.tiles,
        vec!["http://localhost:8080/tiles/rpc/public.function_source/{z}/{x}/{y}.pbf?token=martin"]
    );
}

#[actix_rt::test]
async fn test_get_function_source_tile_ok() {
    init();

    let state = dev::mock_state(None, Some(dev::mock_default_function_sources()), None).await;
    let app = test::init_service(App::new().app_data(Data::new(state)).configure(router)).await;

    let req = test::TestRequest::get()
        .uri("/rpc/public.function_source/0/0/0.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn test_get_function_source_tile_minmax_zoom_ok() {
    init();

    let function_source1 = FunctionSource {
        id: "public.function_source1".to_owned(),
        schema: "public".to_owned(),
        function: "function_source".to_owned(),
        minzoom: None,
        maxzoom: None,
        bounds: Some(Bounds::MAX),
    };

    let function_source2 = FunctionSource {
        id: "public.function_source2".to_owned(),
        schema: "public".to_owned(),
        function: "function_source".to_owned(),
        minzoom: Some(6),
        maxzoom: Some(12),
        bounds: Some(Bounds::MAX),
    };

    let state = dev::mock_state(
        None,
        Some(dev::mock_function_sources(vec![
            function_source1,
            function_source2,
        ])),
        None,
    )
    .await;

    let app = test::init_service(App::new().app_data(Data::new(state)).configure(router)).await;

    // zoom = 0 (public.function_source1)
    let req = test::TestRequest::get()
        .uri("/rpc/public.function_source1/0/0/0.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 6 (public.function_source1)
    let req = test::TestRequest::get()
        .uri("/rpc/public.function_source1/6/38/20.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 12 (public.function_source1)
    let req = test::TestRequest::get()
        .uri("/rpc/public.function_source1/12/2476/1280.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 13 (public.function_source1)
    let req = test::TestRequest::get()
        .uri("/rpc/public.function_source1/13/4952/2560.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 0 (nothing)
    let req = test::TestRequest::get()
        .uri("/rpc/public.function_source2/0/0/0.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert_eq!(response.status(), http::StatusCode::NOT_FOUND);

    // zoom = 6 (public.function_source2)
    let req = test::TestRequest::get()
        .uri("/rpc/public.function_source2/6/38/20.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 12 (public.function_source2)
    let req = test::TestRequest::get()
        .uri("/rpc/public.function_source2/12/2476/1280.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    // zoom = 13 (nothing)
    let req = test::TestRequest::get()
        .uri("/rpc/public.function_source2/13/4952/2560.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert_eq!(response.status(), http::StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn test_get_function_source_query_params_ok() {
    init();

    let state = dev::mock_state(None, Some(dev::mock_default_function_sources()), None).await;
    let app = test::init_service(App::new().app_data(Data::new(state)).configure(router)).await;

    let req = test::TestRequest::get()
        .uri("/rpc/public.function_source_query_params/0/0/0.pbf")
        .to_request();

    let response = test::call_service(&app, req).await;
    println!("response.status = {:?}", response.status());
    assert!(response.status().is_server_error());

    let req = test::TestRequest::get()
        .uri("/rpc/public.function_source_query_params/0/0/0.pbf?token=martin")
        .to_request();

    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn test_get_health_returns_ok() {
    init();

    let state = dev::mock_state(None, Some(dev::mock_default_function_sources()), None).await;
    let app = test::init_service(App::new().app_data(Data::new(state)).configure(router)).await;

    let req = test::TestRequest::get().uri("/healthz").to_request();
    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());
}

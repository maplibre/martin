use actix_http::Request;
use actix_web::http::StatusCode;
use actix_web::test::{call_and_read_body_json, call_service, read_body, TestRequest};
use ctor::ctor;
use martin::pg::config::{FunctionInfo, TableInfo};
use martin::srv::server::IndexEntry;
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
        let sources = $sources.await.0;
        let state = crate::utils::mock_app_data(sources).await;
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
async fn get_catalog_ok() {
    let app = create_app!(mock_unconfigured());

    let req = test_get("/catalog");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    let body = read_body(response).await;
    let sources: Vec<IndexEntry> = serde_json::from_slice(&body).unwrap();

    let expected = "table_source";
    assert_eq!(sources.iter().filter(|v| v.id == expected).count(), 1);

    let expected = "function_zxy_query";
    assert_eq!(sources.iter().filter(|v| v.id == expected).count(), 1);
}

#[actix_rt::test]
async fn get_table_source_ok() {
    let mut tables = mock_table_config_map();
    let table = tables.remove("table_source").unwrap();
    let table_source = TableInfo {
        minzoom: Some(0),
        maxzoom: Some(30),
        ..table.clone()
    };
    let bad_srid = TableInfo {
        srid: 3857,
        ..table
    };
    let app = create_app!(mock_sources(
        None,
        Some(vec![("table_source", table_source), ("bad_srid", bad_srid)]),
        None
    ));

    let req = test_get("/non_existent");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = test_get("/bad_srid");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = TestRequest::get()
        .uri("/table_source?token=martin")
        .insert_header(("x-rewrite-url", "/tiles/table_source?token=martin"))
        .to_request();
    let result: TileJSON = call_and_read_body_json(&app, req).await;
    assert_eq!(result.name, Some(String::from("public.table_source.geom")));
    let expected_uri = "http://localhost:8080/tiles/table_source/{z}/{x}/{y}?token=martin";
    assert_eq!(result.tiles, &[expected_uri]);
    assert_eq!(result.minzoom, Some(0));
    assert_eq!(result.maxzoom, Some(30));
    assert_eq!(result.bounds, Some(Bounds::MAX));
}

#[actix_rt::test]
async fn get_table_source_tile_ok() {
    let app = create_app!(mock_configured_tables(None));

    let req = test_get("/non_existent/0/0/0");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = test_get("/table_source/0/0/0");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn get_table_source_multiple_geom_tile_ok() {
    let app = create_app!(mock_configured_tables(None));

    let req = test_get("/table_source_multiple_geom.geom1/0/0/0");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    let req = test_get("/table_source_multiple_geom.geom2/0/0/0");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn get_table_source_tile_minmax_zoom_ok() {
    let mut tables = mock_table_config_map();

    let app = create_app!(mock_sources(
        None,
        Some(vec![
            (
                "points1",
                TableInfo {
                    minzoom: Some(6),
                    maxzoom: Some(12),
                    ..tables.remove("points1").unwrap()
                },
            ),
            ("points2", tables.remove("points2").unwrap()),
            (
                "points3857",
                TableInfo {
                    minzoom: Some(6),
                    ..tables.remove("points3857").unwrap()
                },
            ),
            (
                "table_source",
                TableInfo {
                    maxzoom: Some(6),
                    ..tables.remove("table_source").unwrap()
                },
            ),
        ]),
        None
    ));

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
async fn get_function_tiles() {
    let app = create_app!(mock_unconfigured());

    let req = test_get("/function_zoom_xy/6/38/20");
    assert!(call_service(&app, req).await.status().is_success());

    let req = test_get("/function_zxy/6/38/20");
    assert!(call_service(&app, req).await.status().is_success());

    let req = test_get("/function_zxy2/6/38/20");
    assert!(call_service(&app, req).await.status().is_success());

    let req = test_get("/function_zxy_query/6/38/20");
    assert!(call_service(&app, req).await.status().is_success());

    let req = test_get("/function_zxy_row/6/38/20");
    assert!(call_service(&app, req).await.status().is_success());

    let req = test_get("/function_Mixed_Name/6/38/20");
    assert!(call_service(&app, req).await.status().is_success());

    let req = test_get("/function_zxy_row_key/6/38/20");
    assert!(call_service(&app, req).await.status().is_success());
}

#[actix_rt::test]
async fn get_composite_source_ok() {
    let app = create_app!(mock_configured_tables(None));

    let req = test_get("/non_existent1,non_existent2");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = test_get("/points1,points2,points3857");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn get_composite_source_tile_ok() {
    let app = create_app!(mock_configured_tables(None));

    let req = test_get("/non_existent1,non_existent2/0/0/0");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = test_get("/points1,points2,points3857/0/0/0");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn get_composite_source_tile_minmax_zoom_ok() {
    let mut tables = mock_table_config_map();

    let points1 = TableInfo {
        minzoom: Some(6),
        maxzoom: Some(13),
        ..tables.remove("points1").unwrap()
    };
    let points2 = TableInfo {
        minzoom: Some(13),
        maxzoom: Some(20),
        ..tables.remove("points2").unwrap()
    };
    let tables = vec![("points1", points1), ("points2", points2)];
    let app = create_app!(mock_sources(None, Some(tables), None));

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
    let app = create_app!(mock_unconfigured());

    let req = test_get("/non_existent");
    let response = call_service(&app, req).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let req = test_get("/function_zoom_xy");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    let req = test_get("/function_zxy");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    let req = test_get("/function_zxy_query");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    let req = test_get("/function_zxy_query_test");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    let req = test_get("/function_zxy_row");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    let req = test_get("/function_Mixed_Name");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());

    let req = test_get("/function_zxy_row_key");
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
    let app = create_app!(mock_unconfigured());

    let req = test_get("/function_zxy_query/0/0/0");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn get_function_source_tile_minmax_zoom_ok() {
    let function_source1 = FunctionInfo::new("public".to_owned(), "function_zxy_query".to_owned());
    let function_source2 = FunctionInfo::new_extended(
        "public".to_owned(),
        "function_zxy_query".to_owned(),
        6,
        12,
        Bounds::MAX,
    );

    let funcs = vec![
        ("function_source1", function_source1),
        ("function_source2", function_source2),
    ];
    let app = create_app!(mock_sources(Some(funcs), None, None));

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
    let app = create_app!(mock_unconfigured());

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
    let app = create_app!(mock_unconfigured());

    let req = test_get("/health");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
}

#[actix_rt::test]
async fn tables_feature_id() {
    let mut tables = mock_table_config_map();

    let default = tables.remove("MIXPOINTS").unwrap();

    let no_id = TableInfo {
        id_column: None,
        properties: props(&[("TABLE", "text")]),
        ..default.clone()
    };
    let id_only = TableInfo {
        id_column: Some("giD".to_string()),
        properties: props(&[("TABLE", "text")]),
        ..default.clone()
    };
    let id_and_prop = TableInfo {
        id_column: Some("giD".to_string()),
        properties: props(&[("giD", "int4"), ("TABLE", "text")]),
        ..default.clone()
    };
    let prop_only = TableInfo {
        id_column: None,
        properties: props(&[("giD", "int4"), ("TABLE", "text")]),
        ..default.clone()
    };

    let tables = vec![
        ("no_id", no_id),
        ("id_only", id_only),
        ("id_and_prop", id_and_prop),
        ("prop_only", prop_only),
    ];
    let mock = mock_sources(None, Some(tables.clone()), None).await;

    let src = table(&mock, "no_id");
    assert_eq!(src.id_column, None);
    assert_eq!(src.properties.len(), 1);
    // let tj = source(&mock, "no_id").get_tilejson();
    // tj.vector_layers.unwrap().iter().for_each(|vl| {
    //     assert_eq!(vl.id, "no_id");
    //     assert_eq!(vl.fields.len(), 2);
    // });

    let src = table(&mock, "id_only");
    assert_eq!(src.id_column, Some("giD".to_string()));
    assert_eq!(src.properties.len(), 1);

    let src = table(&mock, "id_and_prop");
    assert_eq!(src.id_column, Some("giD".to_string()));
    assert_eq!(src.properties.len(), 2);

    let src = table(&mock, "prop_only");
    assert_eq!(src.id_column, None);
    assert_eq!(src.properties.len(), 2);

    // --------------------------------------------

    let app = create_app!(mock_sources(None, Some(tables.clone()), None));
    for (name, _) in tables.iter() {
        let req = test_get(format!("/{name}/0/0/0").as_str());
        let response = call_service(&app, req).await;
        assert!(response.status().is_success());
    }
}

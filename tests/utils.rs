use actix_web::web::Data;
use log::info;
use martin::config::ConfigBuilder;
use martin::pg::config::{FunctionInfo, PgConfigBuilder, TableInfo};
use martin::pg::db::{configure_db_sources, Pool};
use martin::srv::config::SrvConfigBuilder;
use martin::srv::server::{AppState, Sources};
use std::collections::HashMap;
use std::env;
use tilejson::Bounds;

//
// This file is used by many tests and benchmarks using the #[path] attribute.
// Each function should allow dead_code as they might not be used by a specific test file.
//

#[allow(dead_code)]
pub async fn mock_pool() -> Pool {
    let (_, pool) = mock_sources_pool(None, None).await;
    pool
}

#[allow(dead_code)]
pub async fn mock_sources(
    function_sources: Option<&[(&'static str, FunctionInfo)]>,
    table_sources: Option<&[(&'static str, TableInfo)]>,
) -> Sources {
    mock_sources_pool(function_sources, table_sources).await.0
}

#[allow(dead_code)]
pub async fn mock_sources_pool(
    function_sources: Option<&[(&'static str, FunctionInfo)]>,
    table_sources: Option<&[(&'static str, TableInfo)]>,
) -> (Sources, Pool) {
    let connection_string: String = env::var("DATABASE_URL").unwrap();
    info!("Connecting to {connection_string}");
    let config = ConfigBuilder {
        srv: SrvConfigBuilder {
            keep_alive: None,
            listen_addresses: None,
            worker_processes: None,
        },
        pg: PgConfigBuilder {
            connection_string: Some(connection_string),
            ca_root_file: None,
            danger_accept_invalid_certs: None,
            default_srid: None,
            pool_size: None,
            table_sources: table_sources.map(|s| {
                s.iter()
                    .map(|v| (v.0.to_string(), v.1.clone()))
                    .collect::<HashMap<_, _>>()
            }),
            function_sources: function_sources.map(|s| {
                s.iter()
                    .map(|v| (v.0.to_string(), v.1.clone()))
                    .collect::<HashMap<_, _>>()
            }),
        },
    };
    let mut config = config.finalize().expect("Unable to finalize config");
    configure_db_sources(&mut config)
        .await
        .expect("Unable to configure db sources")
}

#[allow(dead_code)]
pub async fn mock_app_data(sources: Sources) -> Data<AppState> {
    Data::new(AppState { sources })
}

#[allow(dead_code)]
pub async fn mock_default_table_sources() -> Sources {
    let table_source = TableInfo {
        schema: "public".to_owned(),
        table: "table_source".to_owned(),
        id_column: None,
        geometry_column: "geom".to_owned(),
        minzoom: Some(0),
        maxzoom: Some(30),
        bounds: Some(Bounds::MAX),
        srid: 4326,
        extent: Some(4096),
        buffer: Some(64),
        clip_geom: Some(true),
        geometry_type: None,
        properties: HashMap::new(),
    };

    let table_source_multiple_geom1 = TableInfo {
        schema: "public".to_owned(),
        table: "table_source_multiple_geom".to_owned(),
        id_column: None,
        geometry_column: "geom1".to_owned(),
        geometry_type: None,
        properties: HashMap::new(),
        ..table_source
    };

    let table_source_multiple_geom2 = TableInfo {
        schema: "public".to_owned(),
        table: "table_source_multiple_geom".to_owned(),
        id_column: None,
        geometry_column: "geom2".to_owned(),
        geometry_type: None,
        properties: HashMap::new(),
        ..table_source
    };

    let table_source1 = TableInfo {
        schema: "public".to_owned(),
        table: "points1".to_owned(),
        id_column: None,
        geometry_column: "geom".to_owned(),
        geometry_type: None,
        properties: HashMap::new(),
        ..table_source
    };

    let table_source2 = TableInfo {
        schema: "public".to_owned(),
        table: "points2".to_owned(),
        id_column: None,
        geometry_column: "geom".to_owned(),
        geometry_type: None,
        properties: HashMap::new(),
        ..table_source
    };

    let table_source3857 = TableInfo {
        schema: "public".to_owned(),
        table: "points3857".to_owned(),
        id_column: None,
        geometry_column: "geom".to_owned(),
        srid: 3857,
        geometry_type: None,
        properties: HashMap::new(),
        ..table_source
    };

    mock_sources(
        None,
        Some(&[
            ("public.table_source", table_source),
            (
                "public.table_source_multiple_geom.geom1",
                table_source_multiple_geom1,
            ),
            (
                "public.table_source_multiple_geom.geom2",
                table_source_multiple_geom2,
            ),
            ("public.points1", table_source1),
            ("public.points2", table_source2),
            ("public.points3857", table_source3857),
        ]),
    )
    .await
}

#[allow(dead_code)]
pub async fn mock_default_function_sources() -> Sources {
    let function_source = FunctionInfo {
        schema: "public".to_owned(),
        function: "function_source".to_owned(),
        minzoom: Some(0),
        maxzoom: Some(30),
        bounds: Some(Bounds::MAX),
    };

    let function_source_query_params = FunctionInfo {
        schema: "public".to_owned(),
        function: "function_source_query_params".to_owned(),
        ..function_source
    };

    mock_sources(
        Some(&[
            ("public.function_source", function_source),
            (
                "public.function_source_query_params",
                function_source_query_params,
            ),
        ]),
        None,
    )
    .await
}

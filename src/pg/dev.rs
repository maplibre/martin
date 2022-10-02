use crate::pg::db::{setup_connection_pool, Pool};
use crate::pg::function_source::{FunctionSource, FunctionSources};
use crate::pg::table_source::{TableSource, TableSources};
use crate::srv::server::AppState;
use log::info;
use std::collections::HashMap;
use std::env;
use tilejson::Bounds;

pub fn mock_table_sources(sources: Vec<TableSource>) -> TableSources {
    let mut table_sources: TableSources = HashMap::new();
    for source in sources {
        table_sources.insert(source.id.clone(), Box::new(source));
    }

    table_sources
}

pub fn mock_default_table_sources() -> TableSources {
    let table_source = TableSource {
        id: "public.table_source".to_owned(),
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

    let table_source_multiple_geom1 = TableSource {
        id: "public.table_source_multiple_geom.geom1".to_owned(),
        schema: "public".to_owned(),
        table: "table_source_multiple_geom".to_owned(),
        id_column: None,
        geometry_column: "geom1".to_owned(),
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

    let table_source_multiple_geom2 = TableSource {
        id: "public.table_source_multiple_geom.geom2".to_owned(),
        schema: "public".to_owned(),
        table: "table_source_multiple_geom".to_owned(),
        id_column: None,
        geometry_column: "geom2".to_owned(),
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

    let table_source1 = TableSource {
        id: "public.points1".to_owned(),
        schema: "public".to_owned(),
        table: "points1".to_owned(),
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

    let table_source2 = TableSource {
        id: "public.points2".to_owned(),
        schema: "public".to_owned(),
        table: "points2".to_owned(),
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

    let table_source3857 = TableSource {
        id: "public.points3857".to_owned(),
        schema: "public".to_owned(),
        table: "points3857".to_owned(),
        id_column: None,
        geometry_column: "geom".to_owned(),
        minzoom: Some(0),
        maxzoom: Some(30),
        bounds: Some(Bounds::MAX),
        srid: 3857,
        extent: Some(4096),
        buffer: Some(64),
        clip_geom: Some(true),
        geometry_type: None,
        properties: HashMap::new(),
    };

    mock_table_sources(vec![
        table_source,
        table_source_multiple_geom1,
        table_source_multiple_geom2,
        table_source1,
        table_source2,
        table_source3857,
    ])
}

pub fn mock_function_sources(sources: Vec<FunctionSource>) -> FunctionSources {
    let mut function_sources: FunctionSources = HashMap::new();
    for source in sources {
        function_sources.insert(source.id.clone(), Box::new(source));
    }

    function_sources
}

pub fn mock_default_function_sources() -> FunctionSources {
    let function_source = FunctionSource {
        id: "public.function_source".to_owned(),
        schema: "public".to_owned(),
        function: "function_source".to_owned(),
        minzoom: Some(0),
        maxzoom: Some(30),
        bounds: Some(Bounds::MAX),
    };

    let function_source_query_params = FunctionSource {
        id: "public.function_source_query_params".to_owned(),
        schema: "public".to_owned(),
        function: "function_source_query_params".to_owned(),
        minzoom: Some(0),
        maxzoom: Some(30),
        bounds: Some(Bounds::MAX),
    };

    mock_function_sources(vec![function_source, function_source_query_params])
}

pub async fn make_pool() -> Pool {
    let connection_string: String = env::var("DATABASE_URL").unwrap();
    info!("Connecting to {connection_string}");

    let pool = setup_connection_pool(&connection_string, &None, 1, false)
        .await
        .unwrap();
    info!("Connected to {connection_string}");

    pool
}

pub async fn mock_state(
    table_sources: Option<TableSources>,
    function_sources: Option<FunctionSources>,
) -> AppState {
    let pool = make_pool().await;

    AppState {
        pool,
        table_sources: table_sources.unwrap_or_default(),
        function_sources: function_sources.unwrap_or_default(),
    }
}

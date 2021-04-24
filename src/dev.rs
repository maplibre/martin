use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::rc::Rc;

use actix::{Actor, Addr, SyncArbiter};

use crate::coordinator_actor::CoordinatorActor;
use crate::db::setup_connection_pool;
use crate::db_actor::DBActor;
use crate::function_source::{FunctionSource, FunctionSources};
use crate::server::AppState;
use crate::table_source::{TableSource, TableSources};

pub fn mock_table_sources() -> Option<TableSources> {
    let source = TableSource {
        id: "public.table_source".to_owned(),
        schema: "public".to_owned(),
        table: "table_source".to_owned(),
        id_column: None,
        geometry_column: "geom".to_owned(),
        srid: 3857,
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
        srid: 3857,
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
        srid: 3857,
        extent: Some(4096),
        buffer: Some(64),
        clip_geom: Some(true),
        geometry_type: None,
        properties: HashMap::new(),
    };

    let mut table_sources: TableSources = HashMap::new();
    table_sources.insert("public.table_source".to_owned(), Box::new(source));
    table_sources.insert("public.points1".to_owned(), Box::new(table_source1));
    table_sources.insert("public.points2".to_owned(), Box::new(table_source2));
    Some(table_sources)
}

pub fn mock_function_sources() -> Option<FunctionSources> {
    let id = "public.function_source";
    let source = FunctionSource {
        id: id.to_owned(),
        schema: "public".to_owned(),
        function: "function_source".to_owned(),
    };

    let mut function_sources: FunctionSources = HashMap::new();
    function_sources.insert(id.to_owned(), Box::new(source));
    Some(function_sources)
}

pub fn mock_state(
    table_sources: Option<TableSources>,
    function_sources: Option<FunctionSources>,
    watch_mode: bool,
) -> AppState {
    let connection_string: String = env::var("DATABASE_URL").unwrap();
    info!("Connecting to {}", connection_string);

    let pool = setup_connection_pool(&connection_string, Some(1), false).unwrap();
    info!("Connected to {}", connection_string);

    let db = SyncArbiter::start(3, move || DBActor(pool.clone()));
    let coordinator: Addr<_> = CoordinatorActor::default().start();

    let table_sources = Rc::new(RefCell::new(table_sources));
    let function_sources = Rc::new(RefCell::new(function_sources));

    AppState {
        db,
        coordinator,
        table_sources,
        function_sources,
        watch_mode,
    }
}

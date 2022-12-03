use actix_web::web::Data;
use log::info;
use martin::pg::config::{FunctionInfo, PgConfigBuilder};
use martin::pg::config::{PgConfig, TableInfo};
use martin::pg::configurator::resolve_pg_data;
use martin::pg::connection::Pool;
use martin::source::IdResolver;
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
    let res = Pool::new(&mock_config(None, None).await).await;
    res.expect("Failed to create pool")
}

#[allow(dead_code)]
pub async fn mock_sources(
    function_sources: Option<&[(&'static str, FunctionInfo)]>,
    table_sources: Option<&[(&'static str, TableInfo)]>,
) -> Sources {
    let cfg = mock_config(function_sources, table_sources).await;
    let res = resolve_pg_data(cfg, IdResolver::default()).await;
    res.expect("Failed to resolve pg data").0
}

#[allow(dead_code)]
pub async fn mock_config(
    function_sources: Option<&[(&'static str, FunctionInfo)]>,
    table_sources: Option<&[(&'static str, TableInfo)]>,
) -> PgConfig {
    let connection_string: String = env::var("DATABASE_URL").unwrap();
    info!("Connecting to {connection_string}");
    let config = PgConfigBuilder {
        connection_string: Some(connection_string),
        #[cfg(feature = "ssl")]
        ca_root_file: None,
        #[cfg(feature = "ssl")]
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
        // unrecognized: Default::default(),
    };
    config.finalize().expect("Unable to finalize config")
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
        unrecognized: HashMap::new(),
    };

    let table_source_multiple_geom1 = TableInfo {
        schema: "public".to_owned(),
        table: "table_source_multiple_geom".to_owned(),
        id_column: None,
        geometry_column: "geom1".to_owned(),
        geometry_type: None,
        properties: HashMap::new(),
        unrecognized: HashMap::new(),
        ..table_source
    };

    let table_source_multiple_geom2 = TableInfo {
        schema: "public".to_owned(),
        table: "table_source_multiple_geom".to_owned(),
        id_column: None,
        geometry_column: "geom2".to_owned(),
        geometry_type: None,
        properties: HashMap::new(),
        unrecognized: HashMap::new(),
        ..table_source
    };

    let table_source1 = TableInfo {
        schema: "public".to_owned(),
        table: "points1".to_owned(),
        id_column: None,
        geometry_column: "geom".to_owned(),
        geometry_type: None,
        properties: HashMap::new(),
        unrecognized: HashMap::new(),
        ..table_source
    };

    let table_source2 = TableInfo {
        schema: "public".to_owned(),
        table: "points2".to_owned(),
        id_column: None,
        geometry_column: "geom".to_owned(),
        geometry_type: None,
        properties: HashMap::new(),
        unrecognized: HashMap::new(),
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
        unrecognized: HashMap::new(),
        ..table_source
    };

    mock_sources(
        None,
        Some(&[
            ("table_source", table_source),
            (
                "table_source_multiple_geom.geom1",
                table_source_multiple_geom1,
            ),
            (
                "table_source_multiple_geom.geom2",
                table_source_multiple_geom2,
            ),
            ("points1", table_source1),
            ("points2", table_source2),
            ("points3857", table_source3857),
        ]),
    )
    .await
}

#[allow(dead_code)]
pub fn single<T, P>(vec: &[T], mut cb: P) -> Option<&T>
where
    T: Sized,
    P: FnMut(&T) -> bool,
{
    let mut iter = vec.iter().filter(|v| cb(v));
    match iter.next() {
        None => None,
        Some(element) => match iter.next() {
            None => Some(element),
            Some(_) => None,
        },
    }
}

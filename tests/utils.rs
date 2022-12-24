#![allow(clippy::missing_panics_doc)]
#![allow(clippy::redundant_clone)]
#![allow(clippy::unused_async)]

use actix_web::web::Data;
use log::info;
use martin::pg::config::PgConfig;
use martin::pg::config_function::FunctionInfo;
use martin::pg::config_table::TableInfo;
use martin::pg::pool::Pool;
use martin::source::{IdResolver, Source};
use martin::srv::server::{AppState, Sources};
use std::collections::HashMap;
use tilejson::Bounds;

pub use martin::args::environment::Env;
#[path = "../src/utils/test_utils.rs"]
mod test_utils;
#[allow(clippy::wildcard_imports)]
pub use test_utils::*;

//
// This file is used by many tests and benchmarks using the #[path] attribute.
// Each function should allow dead_code as they might not be used by a specific test file.
//

pub type MockSource = (Sources, PgConfig);

#[allow(dead_code)]
pub async fn mock_config(
    functions: Option<Vec<(&'static str, FunctionInfo)>>,
    tables: Option<Vec<(&'static str, TableInfo)>>,
    default_srid: Option<i32>,
) -> PgConfig {
    let Ok(db_url) = std::env::var("DATABASE_URL") else {
        panic!("DATABASE_URL env var is not set. Unable to do integration tests");
    };
    info!("Connecting to {db_url}");
    let config = PgConfig {
        connection_string: Some(db_url),
        default_srid,
        tables: tables.map(|s| {
            s.iter()
                .map(|v| (v.0.to_string(), v.1.clone()))
                .collect::<HashMap<_, _>>()
        }),
        functions: functions.map(|s| {
            s.iter()
                .map(|v| (v.0.to_string(), v.1.clone()))
                .collect::<HashMap<_, _>>()
        }),
        ..Default::default()
    };
    config.finalize().expect("Unable to finalize config")
}

#[allow(dead_code)]
pub async fn mock_empty_config() -> PgConfig {
    mock_config(None, None, None).await
}

#[allow(dead_code)]
pub async fn mock_pool() -> Pool {
    let res = Pool::new(&mock_empty_config().await).await;
    res.expect("Failed to create pool")
}

#[allow(dead_code)]
pub async fn mock_sources(mut config: PgConfig) -> MockSource {
    let res = config.resolve(IdResolver::default()).await;
    let res = res.expect("Failed to resolve pg data");
    (res.0, config)
}

#[allow(dead_code)]
pub async fn mock_app_data(sources: Sources) -> Data<AppState> {
    Data::new(AppState { sources })
}

#[allow(dead_code)]
pub async fn mock_unconfigured() -> MockSource {
    mock_sources(mock_empty_config().await).await
}

#[allow(dead_code)]
pub async fn mock_unconfigured_srid(default_srid: Option<i32>) -> MockSource {
    mock_sources(mock_config(None, None, default_srid).await).await
}

#[allow(dead_code)]
pub async fn mock_configured_funcs() -> MockSource {
    mock_sources(mock_config(mock_func_config(), None, None).await).await
}

#[allow(dead_code)]
pub async fn mock_configured_tables(default_srid: Option<i32>) -> PgConfig {
    mock_config(None, mock_table_config(), default_srid).await
}

#[must_use]
#[allow(clippy::unnecessary_wraps)]
pub fn mock_func_config() -> Option<Vec<(&'static str, FunctionInfo)>> {
    Some(mock_func_config_map().into_iter().collect())
}

#[must_use]
#[allow(clippy::unnecessary_wraps)]
pub fn mock_table_config() -> Option<Vec<(&'static str, TableInfo)>> {
    Some(mock_table_config_map().into_iter().collect())
}

#[must_use]
pub fn mock_func_config_map() -> HashMap<&'static str, FunctionInfo> {
    let default = FunctionInfo::default();
    [
        (
            "function_zxy",
            FunctionInfo {
                schema: "public".to_string(),
                function: "function_zxy".to_string(),
                ..default.clone()
            },
        ),
        (
            "function_zxy_query_test",
            FunctionInfo {
                schema: "public".to_string(),
                function: "function_zxy_query_test".to_string(),
                ..default.clone()
            },
        ),
        (
            "function_zxy_row_key",
            FunctionInfo {
                schema: "public".to_string(),
                function: "function_zxy_row_key".to_string(),
                ..default.clone()
            },
        ),
        (
            "function_zxy_query",
            FunctionInfo {
                schema: "public".to_string(),
                function: "function_zxy_query".to_string(),
                ..default.clone()
            },
        ),
        (
            "function_zxy_row",
            FunctionInfo {
                schema: "public".to_string(),
                function: "function_zxy_row".to_string(),
                ..default.clone()
            },
        ),
        (
            // This function is created with non-lowercase name and field names
            "function_mixed_name",
            FunctionInfo {
                schema: "MixedCase".to_string(),
                function: "function_Mixed_Name".to_string(),
                ..default.clone()
            },
        ),
        (
            "function_zoom_xy",
            FunctionInfo {
                schema: "public".to_string(),
                function: "function_zoom_xy".to_string(),
                ..default.clone()
            },
        ),
        (
            "function_zxy2",
            FunctionInfo {
                schema: "public".to_string(),
                function: "function_zxy2".to_string(),
                ..default.clone()
            },
        ),
    ]
    .into_iter()
    .collect()
}

#[must_use]
#[allow(clippy::too_many_lines)]
pub fn mock_table_config_map() -> HashMap<&'static str, TableInfo> {
    let default = TableInfo {
        srid: 4326,
        minzoom: Some(0),
        maxzoom: Some(30),
        bounds: Some(Bounds::MAX),
        extent: Some(4096),
        buffer: Some(64),
        clip_geom: Some(true),
        ..Default::default()
    };

    [
        (
            "points1",
            TableInfo {
                schema: "public".to_string(),
                table: "points1".to_string(),
                geometry_column: "geom".to_string(),
                geometry_type: some_str("POINT"),
                properties: props(&[("gid", "int4")]),
                ..default.clone()
            },
        ),
        (
            "points2",
            TableInfo {
                schema: "public".to_string(),
                table: "points2".to_string(),
                geometry_column: "geom".to_string(),
                geometry_type: some_str("POINT"),
                properties: props(&[("gid", "int4")]),
                ..default.clone()
            },
        ),
        (
            // This table is created with non-lowercase name and field names
            "MIXPOINTS",
            TableInfo {
                schema: "MIXEDCASE".to_string(),
                table: "mixPoints".to_string(),
                geometry_column: "geoM".to_string(),
                geometry_type: some_str("POINT"),
                id_column: some_str("giD"),
                properties: props(&[("tAble", "text")]),
                ..default.clone()
            },
        ),
        (
            "points3857",
            TableInfo {
                schema: "public".to_string(),
                table: "points3857".to_string(),
                srid: 3857,
                geometry_column: "geom".to_string(),
                geometry_type: some_str("POINT"),
                properties: props(&[("gid", "int4")]),
                ..default.clone()
            },
        ),
        (
            "points_empty_srid",
            TableInfo {
                schema: "public".to_string(),
                table: "points_empty_srid".to_string(),
                srid: 900_973,
                geometry_column: "geom".to_string(),
                geometry_type: some_str("GEOMETRY"),
                properties: props(&[("gid", "int4")]),
                ..default.clone()
            },
        ),
        (
            "table_source",
            TableInfo {
                schema: "public".to_string(),
                table: "table_source".to_string(),
                geometry_column: "geom".to_string(),
                geometry_type: some_str("GEOMETRY"),
                properties: props(&[("gid", "int4")]),
                ..default.clone()
            },
        ),
        (
            "table_source_multiple_geom.geom1",
            TableInfo {
                schema: "public".to_string(),
                table: "table_source_multiple_geom".to_string(),
                geometry_column: "geom1".to_string(),
                geometry_type: some_str("POINT"),
                properties: props(&[("geom2", "geometry"), ("gid", "int4")]),
                ..default.clone()
            },
        ),
        (
            "table_source_multiple_geom.geom2",
            TableInfo {
                schema: "public".to_string(),
                table: "table_source_multiple_geom".to_string(),
                geometry_column: "geom2".to_string(),
                geometry_type: some_str("POINT"),
                properties: props(&[("gid", "int4"), ("geom1", "geometry")]),
                ..default.clone()
            },
        ),
    ]
    .into_iter()
    .collect()
}

#[must_use]
pub fn props(props: &[(&'static str, &'static str)]) -> HashMap<String, String> {
    props
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

#[allow(dead_code)]
#[must_use]
pub fn table<'a>(mock: &'a MockSource, name: &str) -> &'a TableInfo {
    let (_, PgConfig { tables, .. }) = mock;
    tables.as_ref().map(|v| v.get(name).unwrap()).unwrap()
}

#[allow(dead_code)]
#[must_use]
pub fn source<'a>(mock: &'a MockSource, name: &str) -> &'a dyn Source {
    let (sources, _) = mock;
    sources.get(name).unwrap().as_ref()
}

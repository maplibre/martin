use ctor::ctor;
use log::info;
use martin::pg::config::{PgSqlInfo, SqlTableInfoMapMapMap, TableInfo};
use martin::pg::table_source::get_table_sources;
use martin::source::Xyz;
use std::collections::HashMap;

#[path = "utils.rs"]
mod utils;
use utils::*;

#[ctor]
fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[actix_rt::test]
async fn get_table_sources_ok() {
    let pool = mock_pool().await;
    let sources = get_table_sources(&pool, Some(900913)).await.unwrap();
    assert!(!sources.is_empty());

    let (_, source) = get_source(&sources, "table_source", "geom");
    assert_eq!(source.schema, "public");
    assert_eq!(source.table, "table_source");
    assert_eq!(source.srid, 4326);
    assert_eq!(source.geometry_column, "geom");
    assert_eq!(source.id_column, None);
    assert_eq!(source.minzoom, None);
    assert_eq!(source.maxzoom, None);
    assert!(source.bounds.is_some());
    assert_eq!(source.extent, Some(4096));
    assert_eq!(source.buffer, Some(64));
    assert_eq!(source.clip_geom, Some(true));
    assert_eq!(source.geometry_type, Some("GEOMETRY".to_owned()));

    let mut properties = HashMap::new();
    properties.insert("gid".to_owned(), "int4".to_owned());
    assert_eq!(source.properties, properties);
}

#[actix_rt::test]
async fn table_source_tilejson_ok() {
    let sources = mock_sources(None, None).await;
    let source = sources.get("table_source").unwrap();
    let tilejson = source.get_tilejson();

    info!("tilejson = {tilejson:#?}");

    assert_eq!(tilejson.tilejson, "2.2.0");
    assert_eq!(tilejson.version, Some("1.0.0".to_owned()));
    assert_eq!(tilejson.name, Some("public.table_source.geom".to_owned()));
    assert_eq!(tilejson.scheme, Some("xyz".to_owned()));
    assert_eq!(tilejson.minzoom, Some(0));
    assert_eq!(tilejson.maxzoom, Some(30));
    assert!(tilejson.bounds.is_some());
    assert!(tilejson.tiles.is_empty());
}

#[actix_rt::test]
async fn table_source_tile_ok() {
    let sources = mock_sources(None, None).await;
    let source = sources.get("table_source").unwrap();
    let tile = source.get_tile(&Xyz::new(0, 0, 0), &None).await.unwrap();

    assert!(!tile.is_empty());
}

#[actix_rt::test]
async fn table_source_srid_ok() {
    let pool = mock_pool().await;
    let table_sources = get_table_sources(&pool, Some(900913)).await.unwrap();

    let (_, source) = get_source(&table_sources, "points1", "geom");
    assert_eq!(source.srid, 4326);

    let (_, source) = get_source(&table_sources, "points2", "geom");
    assert_eq!(source.srid, 4326);

    let (_, source) = get_source(&table_sources, "points3857", "geom");
    assert_eq!(source.srid, 3857);

    let (_, source) = get_source(&table_sources, "points_empty_srid", "geom");
    assert_eq!(source.srid, 900913);
}

#[actix_rt::test]
async fn table_source_multiple_geom_ok() {
    let pool = mock_pool().await;
    let sources = get_table_sources(&pool, Some(900913)).await.unwrap();

    let (_, source) = get_source(&sources, "table_source_multiple_geom", "geom1");
    assert_eq!(source.geometry_column, "geom1");

    let (_, source) = get_source(&sources, "table_source_multiple_geom", "geom2");
    assert_eq!(source.geometry_column, "geom2");
}

fn get_source<'a>(
    sources: &'a SqlTableInfoMapMapMap,
    name: &'static str,
    geom: &'static str,
) -> &'a (PgSqlInfo, TableInfo) {
    let srcs = sources.get("public").expect("public schema not found");
    let cols = srcs
        .get(name)
        .unwrap_or_else(|| panic!("table {name} not found"));
    cols.get(geom)
        .unwrap_or_else(|| panic!("table {name}.{geom} not found"))
}

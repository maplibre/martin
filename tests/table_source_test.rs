use ctor::ctor;
use log::info;
use martin::pg::config::{TableInfo, TableInfoSources, TableInfoVec};
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
    let table_sources = get_table_sources(&pool, &TableInfoSources::default(), None)
        .await
        .unwrap();

    info!("table_sources = {table_sources:#?}");

    assert!(!table_sources.is_empty());
    let table_source = get_source(&table_sources, "table_source");
    assert_eq!(table_source.schema, "public");
    assert_eq!(table_source.table, "table_source");
    assert_eq!(table_source.srid, 4326);
    assert_eq!(table_source.geometry_column, "geom");
    assert_eq!(table_source.id_column, None);
    assert_eq!(table_source.minzoom, None);
    assert_eq!(table_source.maxzoom, None);
    assert!(table_source.bounds.is_some());
    assert_eq!(table_source.extent, Some(4096));
    assert_eq!(table_source.buffer, Some(64));
    assert_eq!(table_source.clip_geom, Some(true));
    assert_eq!(table_source.geometry_type, Some("GEOMETRY".to_owned()));

    let mut properties = HashMap::new();
    properties.insert("gid".to_owned(), "int4".to_owned());
    assert_eq!(table_source.properties, properties);
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
    let tile = source
        .get_tile(&Xyz { x: 0, y: 0, z: 0 }, &None)
        .await
        .unwrap();

    assert!(!tile.is_empty());
}

#[actix_rt::test]
async fn table_source_srid_ok() {
    let pool = mock_pool().await;
    let table_sources = get_table_sources(&pool, &TableInfoSources::default(), Some(900913))
        .await
        .unwrap();

    let points1 = get_source(&table_sources, "points1");
    assert_eq!(points1.srid, 4326);

    let points2 = get_source(&table_sources, "points2");
    assert_eq!(points2.srid, 4326);

    let points3857 = get_source(&table_sources, "points3857");
    assert_eq!(points3857.srid, 3857);

    let points_empty_srid = get_source(&table_sources, "points_empty_srid");
    assert_eq!(points_empty_srid.srid, 900913);
}

#[actix_rt::test]
async fn table_source_multiple_geom_ok() {
    let pool = mock_pool().await;
    let table_sources = get_table_sources(&pool, &TableInfoSources::default(), None)
        .await
        .unwrap();

    let table_source_multiple_geom = single(&table_sources, |v| {
        v.table == "table_source_multiple_geom" && v.geometry_column == "geom1"
    })
    .expect("table_source_multiple_geom.geom1 not found");
    assert_eq!(table_source_multiple_geom.geometry_column, "geom1");

    let table_source_multiple_geom = single(&table_sources, |v| {
        v.table == "table_source_multiple_geom" && v.geometry_column == "geom2"
    })
    .expect("table_source_multiple_geom.geom2 not found");
    assert_eq!(table_source_multiple_geom.geometry_column, "geom2");
}

fn get_source<'a>(table_sources: &'a TableInfoVec, name: &'static str) -> &'a TableInfo {
    single(table_sources, |v| v.table == *name).unwrap_or_else(|| panic!("{name} not found"))
}

use std::collections::HashMap;

use martin::dev;
use martin::source::{Source, Xyz};
use martin::table_source::get_table_sources;

fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[actix_rt::test]
async fn test_get_table_sources_ok() {
    init();

    let mut connection = dev::make_pool().get().unwrap();
    let table_sources = get_table_sources(&mut connection, &None).unwrap();

    log::info!("table_sources = {table_sources:#?}");

    assert!(!table_sources.is_empty());
    assert!(table_sources.contains_key("public.table_source"));

    let table_source = table_sources.get("public.table_source").unwrap();
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
async fn test_table_source_tilejson_ok() {
    init();

    let mut connection = dev::make_pool().get().unwrap();
    let table_sources = get_table_sources(&mut connection, &None).unwrap();

    let table_source = table_sources.get("public.table_source").unwrap();
    let tilejson = table_source.get_tilejson().unwrap();

    log::info!("tilejson = {tilejson:#?}");

    assert_eq!(tilejson.tilejson, "2.2.0");
    assert_eq!(tilejson.version, Some("1.0.0".to_owned()));
    assert_eq!(tilejson.name, Some("public.table_source".to_owned()));
    assert_eq!(tilejson.scheme, Some("xyz".to_owned()));
    assert_eq!(tilejson.minzoom, Some(0));
    assert_eq!(tilejson.maxzoom, Some(30));
    assert!(tilejson.bounds.is_some());
    assert!(tilejson.tiles.is_empty());
}

#[actix_rt::test]
async fn test_table_source_tile_ok() {
    init();

    let mut connection = dev::make_pool().get().unwrap();
    let table_sources = get_table_sources(&mut connection, &None).unwrap();

    let table_source = table_sources.get("public.table_source").unwrap();
    let tile = table_source
        .get_tile(&mut connection, &Xyz { x: 0, y: 0, z: 0 }, &None)
        .unwrap();

    assert!(!tile.is_empty());
}

#[actix_rt::test]
async fn test_table_source_srid_ok() {
    init();

    let mut connection = dev::make_pool().get().unwrap();
    let table_sources = get_table_sources(&mut connection, &Some(900913)).unwrap();

    assert!(table_sources.contains_key("public.points1"));
    let points1 = table_sources.get("public.points1").unwrap();
    assert_eq!(points1.srid, 4326);

    assert!(table_sources.contains_key("public.points2"));
    let points2 = table_sources.get("public.points2").unwrap();
    assert_eq!(points2.srid, 4326);

    assert!(table_sources.contains_key("public.points3857"));
    let points3857 = table_sources.get("public.points3857").unwrap();
    assert_eq!(points3857.srid, 3857);

    assert!(table_sources.contains_key("public.points_empty_srid"));
    let points_empty_srid = table_sources.get("public.points_empty_srid").unwrap();
    assert_eq!(points_empty_srid.srid, 900913);
}

#[actix_rt::test]
async fn test_table_source_multiple_geom_ok() {
    init();

    let mut connection = dev::make_pool().get().unwrap();
    let table_sources = get_table_sources(&mut connection, &None).unwrap();

    assert!(table_sources.contains_key("public.table_source_multiple_geom"));
    let table_source_multiple_geom = table_sources
        .get("public.table_source_multiple_geom")
        .unwrap();

    assert_eq!(table_source_multiple_geom.geometry_column, "geom1");

    assert!(table_sources.contains_key("public.table_source_multiple_geom.geom1"));
    let table_source_multiple_geom1 = table_sources
        .get("public.table_source_multiple_geom.geom1")
        .unwrap();

    assert_eq!(table_source_multiple_geom1.geometry_column, "geom1");

    assert!(table_sources.contains_key("public.table_source_multiple_geom.geom2"));
    let table_source_multiple_geom2 = table_sources
        .get("public.table_source_multiple_geom.geom2")
        .unwrap();

    assert_eq!(table_source_multiple_geom2.geometry_column, "geom2");
}

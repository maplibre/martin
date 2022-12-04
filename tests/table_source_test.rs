use ctor::ctor;
use log::info;
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
async fn table_sources() {
    let mock = mock_unconfigured().await;
    assert!(!mock.0.is_empty());

    let source = table(&mock, "table_source");
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
    let mock = mock_unconfigured().await;
    let tilejson = source(&mock, "table_source").get_tilejson();

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
    let mock = mock_unconfigured().await;
    let src = source(&mock, "table_source");
    let tile = src.get_tile(&Xyz::new(0, 0, 0), &None).await.unwrap();

    assert!(!tile.is_empty());
}

#[actix_rt::test]
async fn table_source_srid_ok() {
    let mock = mock_unconfigured_srid(Some(900913)).await;

    dbg!(&mock);

    let source = table(&mock, "points1");
    assert_eq!(source.srid, 4326);

    let source = table(&mock, "points2");
    assert_eq!(source.srid, 4326);

    let source = table(&mock, "points3857");
    assert_eq!(source.srid, 3857);

    let source = table(&mock, "points_empty_srid");
    assert_eq!(source.srid, 900913);
}

#[actix_rt::test]
async fn table_source_multiple_geom_ok() {
    let mock = mock_unconfigured().await;

    let source = table(&mock, "table_source_multiple_geom");
    assert_eq!(source.geometry_column, "geom1");

    let source = table(&mock, "table_source_multiple_geom.1");
    assert_eq!(source.geometry_column, "geom2");
}

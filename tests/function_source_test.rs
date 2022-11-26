use ctor::ctor;
use log::info;
use martin::pg::function_source::get_function_sources as get_sources;
use martin::source::Xyz;

#[path = "utils.rs"]
mod utils;
use utils::*;

#[ctor]
fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[actix_rt::test]
async fn get_function_sources() {
    let pool = mock_pool().await;
    let sources = get_sources(&pool).await.unwrap();

    // dbg!(&sources);
    assert!(!sources.is_empty());

    let funcs = sources.get("public").expect("public schema not found");
    let source = funcs
        .get("function_zxy_query")
        .expect("function_zxy_query not found");
    assert_eq!(source.info.schema, "public");
    assert_eq!(source.info.function, "function_zxy_query");
    assert_eq!(source.info.minzoom, None);
    assert_eq!(source.info.maxzoom, None);
    assert_eq!(source.info.bounds, None);
}

#[actix_rt::test]
async fn function_source_tilejson() {
    let sources = mock_sources(None, None).await;
    let source = sources.get("function_zxy_query").unwrap();
    let tilejson = source.get_tilejson();

    info!("tilejson = {tilejson:#?}");

    assert_eq!(tilejson.tilejson, "2.2.0");
    assert_eq!(tilejson.version, Some("1.0.0".to_owned()));
    assert_eq!(tilejson.name, Some("public.function_zxy_query".to_owned()));
    assert_eq!(tilejson.scheme, Some("xyz".to_owned()));
    assert_eq!(tilejson.minzoom, Some(0));
    assert_eq!(tilejson.maxzoom, Some(30));
    assert!(tilejson.bounds.is_some());
    assert!(tilejson.tiles.is_empty());
}

#[actix_rt::test]
async fn function_source_tile() {
    let sources = mock_sources(None, None).await;
    let source = sources.get("function_zxy_query").unwrap();
    let tile = source
        .get_tile(&Xyz { x: 0, y: 0, z: 0 }, &None)
        .await
        .unwrap();

    assert!(!tile.is_empty());
}

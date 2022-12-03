use ctor::ctor;
use log::info;
use martin::pg::function_source::get_function_sources;
use martin::source::Xyz;

#[path = "utils.rs"]
mod utils;
use utils::*;

#[ctor]
fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[actix_rt::test]
async fn get_function_sources_ok() {
    let pool = mock_pool().await;
    let sources = get_function_sources(&pool).await.unwrap();

    assert!(!sources.is_empty());

    let funcs = sources.get("public").expect("public schema not found");
    let source = funcs
        .get("function_zxy_query")
        .expect("function_zxy_query not found");
    assert_eq!(source.1.schema, "public");
    assert_eq!(source.1.function, "function_zxy_query");
    assert_eq!(source.1.minzoom, None);
    assert_eq!(source.1.maxzoom, None);
    assert_eq!(source.1.bounds, None);
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
    let tile = source.get_tile(&Xyz::new(0, 0, 0), &None).await.unwrap();

    assert!(!tile.is_empty());
}

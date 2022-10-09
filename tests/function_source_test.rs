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
    let function_sources = get_sources(&pool).await.unwrap();

    info!("function_sources = {function_sources:#?}");

    assert!(!function_sources.is_empty());
    assert!(function_sources.contains_key("public.function_source"));

    let function_source = function_sources.get("public.function_source").unwrap();
    assert_eq!(function_source.schema, "public");
    assert_eq!(function_source.function, "function_source");
    assert_eq!(function_source.minzoom, None);
    assert_eq!(function_source.maxzoom, None);
    assert_eq!(function_source.bounds, None);
}

#[actix_rt::test]
async fn function_source_tilejson() {
    let sources = mock_sources(None, None).await;
    let source = sources.get("public.function_source").unwrap();
    let tilejson = source.get_tilejson();

    info!("tilejson = {tilejson:#?}");

    assert_eq!(tilejson.tilejson, "2.2.0");
    assert_eq!(tilejson.version, Some("1.0.0".to_owned()));
    assert_eq!(tilejson.name, Some("public.function_source".to_owned()));
    assert_eq!(tilejson.scheme, Some("xyz".to_owned()));
    assert_eq!(tilejson.minzoom, Some(0));
    assert_eq!(tilejson.maxzoom, Some(30));
    assert!(tilejson.bounds.is_some());
    assert!(tilejson.tiles.is_empty());
}

#[actix_rt::test]
async fn function_source_tile() {
    let sources = mock_sources(None, None).await;
    let source = sources.get("public.function_source").unwrap();
    let tile = source
        .get_tile(&Xyz { x: 0, y: 0, z: 0 }, &None)
        .await
        .unwrap();

    assert!(!tile.is_empty());
}

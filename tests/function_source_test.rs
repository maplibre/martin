use martin::pg::dev;
use martin::pg::function_source::get_function_sources;
use martin::source::{Source, Xyz};

fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[actix_rt::test]
async fn test_get_function_sources_ok() {
    init();

    let pool = dev::make_pool().await;
    let mut connection = pool.get().await.unwrap();
    let function_sources = get_function_sources(&mut connection).await.unwrap();

    log::info!("function_sources = {function_sources:#?}");

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
async fn test_function_source_tilejson_ok() {
    init();

    let pool = dev::make_pool().await;
    let mut connection = pool.get().await.unwrap();
    let function_sources = get_function_sources(&mut connection).await.unwrap();

    let function_source = function_sources.get("public.function_source").unwrap();
    let tilejson = function_source.get_tilejson().await.unwrap();

    log::info!("tilejson = {tilejson:#?}");

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
async fn test_function_source_tile_ok() {
    init();

    let pool = dev::make_pool().await;
    let mut connection = pool.get().await.unwrap();
    let function_sources = get_function_sources(&mut connection).await.unwrap();

    let function_source = function_sources.get("public.function_source").unwrap();
    let tile = function_source
        .get_tile(&mut connection, &Xyz { x: 0, y: 0, z: 0 }, &None)
        .await
        .unwrap();

    assert!(!tile.is_empty());
}

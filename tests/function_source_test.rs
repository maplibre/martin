use ctor::ctor;
use itertools::Itertools;
use log::info;
use martin::pg::{get_function_sources, Schemas};
use martin::Xyz;

#[path = "utils.rs"]
mod utils;
#[allow(clippy::wildcard_imports)]
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
    let mock = mock_unconfigured().await;
    let tilejson = source(&mock, "function_zxy_query").get_tilejson();

    info!("tilejson = {tilejson:#?}");

    assert_eq!(tilejson.tilejson, "2.2.0");
    assert_eq!(tilejson.version, some_str("1.0.0"));
    assert_eq!(tilejson.name, some_str("public.function_zxy_query"));
    assert_eq!(tilejson.scheme, some_str("xyz"));
    assert_eq!(tilejson.minzoom, Some(0));
    assert_eq!(tilejson.maxzoom, Some(30));
    assert!(tilejson.bounds.is_some());
    assert!(tilejson.tiles.is_empty());
}

#[actix_rt::test]
async fn function_source_tile() {
    let mock = mock_unconfigured().await;
    let src = source(&mock, "function_zxy_query");
    let tile = src
        .get_tile(&Xyz { z: 0, x: 0, y: 0 }, &None)
        .await
        .unwrap();

    assert!(!tile.is_empty());
}

#[actix_rt::test]
async fn function_source_schemas() {
    let mut cfg = mock_empty_config().await;
    cfg.auto_functions = Some(Schemas::List(vec!["MixedCase".to_owned()]));
    cfg.auto_tables = Some(Schemas::Bool(false));
    let sources = mock_sources(cfg).await.0;
    assert_eq!(
        sources.keys().sorted().collect::<Vec<_>>(),
        vec!["function_Mixed_Name"],
    );
}

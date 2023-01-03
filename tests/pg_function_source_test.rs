use ctor::ctor;
use indoc::indoc;
use itertools::Itertools;
use martin::pg::get_function_sources;
use martin::Xyz;

pub mod utils;
pub use utils::*;

#[ctor]
fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[actix_rt::test]
async fn get_function_sources_ok() {
    let pool = mock_pool().await;
    let sources = get_function_sources(&pool).await.unwrap();

    assert!(!sources.is_empty());

    let funcs = sources.get("public").unwrap();
    let source = funcs.get("function_zxy_query").unwrap();
    assert_eq!(source.1.schema, "public");
    assert_eq!(source.1.function, "function_zxy_query");
    assert_eq!(source.1.minzoom, None);
    assert_eq!(source.1.maxzoom, None);
    assert_eq!(source.1.bounds, None);

    let source = funcs.get("function_zxy_query_jsonb").unwrap();
    assert_eq!(source.1.schema, "public");
    assert_eq!(source.1.function, "function_zxy_query_jsonb");
}

#[actix_rt::test]
async fn function_source_tilejson() {
    let mock = mock_sources(mock_pgcfg("connection_string: $DATABASE_URL")).await;
    let tilejson = source(&mock, "function_zxy_query").get_tilejson();

    assert_eq!(tilejson.tilejson, "2.2.0");
    assert_eq!(tilejson.version, some("1.0.0"));
    assert_eq!(tilejson.name, some("public.function_zxy_query"));
    assert_eq!(tilejson.scheme, some("xyz"));
    assert_eq!(tilejson.minzoom, Some(0));
    assert_eq!(tilejson.maxzoom, Some(30));
    assert!(tilejson.bounds.is_some());
    assert!(tilejson.tiles.is_empty());
}

#[actix_rt::test]
async fn function_source_tile() {
    let mock = mock_sources(mock_pgcfg("connection_string: $DATABASE_URL")).await;
    let src = source(&mock, "function_zxy_query");
    let tile = src
        .get_tile(&Xyz { z: 0, x: 0, y: 0 }, &None)
        .await
        .unwrap();
    assert!(!tile.is_empty());

    let src = source(&mock, "function_zxy_query_jsonb");
    let tile = src
        .get_tile(&Xyz { z: 0, x: 0, y: 0 }, &None)
        .await
        .unwrap();
    assert!(!tile.is_empty());
}

#[actix_rt::test]
async fn function_source_schemas() {
    let cfg = mock_pgcfg(indoc! {"
        connection_string: $DATABASE_URL
        auto_publish:
          tables: false
          functions:
            from_schemas: MixedCase
    "});
    let sources = mock_sources(cfg).await.0;
    assert_eq!(
        sources.keys().sorted().collect::<Vec<_>>(),
        vec!["function_Mixed_Name"],
    );
}

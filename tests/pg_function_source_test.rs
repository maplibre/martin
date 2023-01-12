use ctor::ctor;
use indoc::indoc;
use itertools::Itertools;
use martin::Xyz;

pub mod utils;
pub use utils::*;

#[ctor]
fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[actix_rt::test]
async fn function_source_tilejson() {
    let mock = mock_sources(mock_pgcfg("connection_string: $DATABASE_URL")).await;
    let tilejson = source(&mock, "function_zxy_query").get_tilejson();

    assert_eq!(tilejson.tilejson, "3.0.0");
    assert_eq!(tilejson.name, some("public.function_zxy_query"));
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

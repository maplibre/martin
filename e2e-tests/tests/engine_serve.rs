//! Table sources with `layers`, served by Martin's engine.
#![cfg(all(feature = "test-pg", feature = "test-generate"))]

use indoc::indoc;
use martin_e2e_tests::Martin;

#[tokio::test]
async fn a_miss_caches_the_rest_of_its_prefetch_block() {
    let mut martin = Martin::builder()
        .with_postgres()
        .config(indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                table_source:
                  schema: public
                  table: table_source
                  filter: \"gid <> 14\"
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
                  id_column: gid
                  maxzoom: 3
                  prefetch: 2
                  properties:
                    gid: int4
                  layers:
                    mixed: {}
        "})
        .start()
        .await
        .expect("failed to start martin");

    let mut lines = Vec::new();
    for path in [
        "/table_source/1/0/0",
        "/table_source/1/1/0",
        "/table_source/1/0/1",
        "/table_source/1/1/1",
        "/table_source/2/0/0",
        "/table_source/2/1/1",
    ] {
        lines.push(format!("{path} {}", martin.get(path).await.status()));
    }

    let scrape = martin.get("/_/metrics").await.text();
    lines.extend(
        scrape
            .lines()
            .filter(|line| line.starts_with("martin_tile_cache_requests_total"))
            .map(str::to_owned),
    );
    insta::assert_snapshot!(lines.join("\n"), @r#"
    /table_source/1/0/0 200
    /table_source/1/1/0 200
    /table_source/1/0/1 200
    /table_source/1/1/1 200
    /table_source/2/0/0 204
    /table_source/2/1/1 200
    martin_tile_cache_requests_total{cache="tile",result="hit",zoom="1"} 3
    martin_tile_cache_requests_total{cache="tile",result="hit",zoom="2"} 1
    martin_tile_cache_requests_total{cache="tile",result="miss",zoom="1"} 1
    martin_tile_cache_requests_total{cache="tile",result="miss",zoom="2"} 1
    "#);
    martin.stop().await;
    martin.assert_log_contains(
        "Table has no spatial index on the geometry column schema=public table=table_source geometry_column=geom",
    );
}

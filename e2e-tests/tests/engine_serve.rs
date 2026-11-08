//! Table sources with `layers`, served by Martin's engine.
#![cfg(feature = "test-pg")]

use std::fs;

use indoc::indoc;
use martin_e2e_tests::{Martin, MartinCp, temp_dir, tiles};

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

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "inline configs of the bulk run and both servers")]
async fn served_per_tile_tiles_equal_bulk_tiles_with_and_without_prefetch() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
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
                  buffer: 1024
                  maxzoom: 3
                  properties:
                    gid: int4
                  layers:
                    merged:
                      attributes:
                        kind: 'gid % 2'
                      sort_by: [gid]
                      limit: 5
        "},
    )
    .expect("failed to write the config");
    let output = dir.path().join("bulk.mbtiles");
    MartinCp::bulk()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .arg("--format")
        .arg("mvt")
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
        .run()
        .await;
    let bulk = tiles(&output).await;

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
                  buffer: 1024
                  maxzoom: 3
                  properties:
                    gid: int4
                  layers:
                    merged:
                      attributes:
                        kind: 'gid % 2'
                      sort_by: [gid]
                      limit: 5
        "})
        .start()
        .await
        .expect("failed to start martin");
    for (z, x, y, data) in &bulk {
        let row = (1_i64 << z) - 1 - y;
        let served = martin.get(&format!("/table_source/{z}/{x}/{row}")).await;
        assert_eq!(served.body(), data.as_slice(), "{z}/{x}/{row} without prefetch");
    }
    martin.stop().await;
    martin.assert_log_contains("Table has no spatial index on the geometry column");

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
                  buffer: 1024
                  maxzoom: 3
                  prefetch: 2
                  properties:
                    gid: int4
                  layers:
                    merged:
                      attributes:
                        kind: 'gid % 2'
                      sort_by: [gid]
                      limit: 5
        "})
        .start()
        .await
        .expect("failed to start martin");
    for (z, x, y, data) in &bulk {
        let row = (1_i64 << z) - 1 - y;
        let served = martin.get(&format!("/table_source/{z}/{x}/{row}")).await;
        assert_eq!(served.body(), data.as_slice(), "{z}/{x}/{row} with prefetch");
    }
    martin.stop().await;
    martin.assert_log_contains("Table has no spatial index on the geometry column");
}

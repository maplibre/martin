//! The `martin generate` bulk tile generator.
#![cfg(all(feature = "test-pg", feature = "test-generate"))]

use std::fs;

use indoc::indoc;
use martin_e2e_tests::{
    MartinCp, MbtilesCli, assert_pmtiles_matches_mbtiles, metadata_listing, temp_dir, tile_listing,
};

#[tokio::test]
async fn generates_a_valid_tileset() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                points1:
                  schema: public
                  table: points1
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  properties:
                    gid: int4
                table_source:
                  schema: public
                  table: table_source
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
                  properties:
                    gid: int4
                  layer_id: mixed
        "},
    )
    .expect("failed to write the config");
    let output = dir.path().join("generated.mbtiles");
    MartinCp::generate()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
        .arg("--threads")
        .arg("2")
        .run()
        .await;
    MbtilesCli::new("validate").arg(&output).run().await;
    insta::assert_snapshot!("tiles", tile_listing(&output).await);
    let metadata = metadata_listing(&output).await;
    insta::with_settings!({ filters => vec![(r"martin generate v[0-9.]+[^\s]*", "martin generate v[VERSION]")] }, {
        insta::assert_snapshot!("metadata", metadata);
    });

    // The output does not depend on the thread count or memory.
    let again = dir.path().join("again.mbtiles");
    MartinCp::generate()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&again)
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
        .arg("--threads")
        .arg("5")
        .arg("--memory")
        .arg("64MiB")
        .run()
        .await;
    assert_eq!(
        martin_e2e_tests::tiles(&again).await,
        martin_e2e_tests::tiles(&output).await
    );
}

#[tokio::test]
async fn generates_pmtiles_with_the_same_tiles() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                points1:
                  schema: public
                  table: points1
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  properties:
                    gid: int4
                table_source:
                  schema: public
                  table: table_source
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
                  properties:
                    gid: int4
                  layer_id: mixed
        "},
    )
    .expect("failed to write the config");
    let (mbtiles, pmtiles) = (
        dir.path().join("out.mbtiles"),
        dir.path().join("out.pmtiles"),
    );
    MartinCp::generate()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&mbtiles)
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
        .run()
        .await;
    MartinCp::generate()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&pmtiles)
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
        .run()
        .await;
    assert_pmtiles_matches_mbtiles(&pmtiles, &mbtiles).await;

    // An existing archive is neither overwritten nor removed.
    let log = MartinCp::generate()
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&pmtiles)
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
        .run_expecting_failure()
        .await;
    assert!(log.contains("not empty"), "{log}");
    assert_pmtiles_matches_mbtiles(&pmtiles, &mbtiles).await;
}

#[tokio::test]
async fn rejects_function_sources() {
    let dir = temp_dir();
    let log = MartinCp::generate()
        .with_postgres()
        .arg("--output-file")
        .arg(dir.path().join("out.mbtiles"))
        .arg("--source")
        .arg("function_zxy_query")
        .run_expecting_failure()
        .await;
    assert!(log.contains("function source"), "{log}");
}

#[tokio::test]
async fn rejects_inverted_and_off_world_bboxes() {
    let dir = temp_dir();
    for (bbox, expected) in [
        (
            "--bbox=170,-50,-170,-30",
            "has a minimum greater than its maximum",
        ),
        ("--bbox=-10,-89,10,89", "latitude"),
    ] {
        let log = MartinCp::generate()
            .with_postgres()
            .arg("--output-file")
            .arg(dir.path().join("out.mbtiles"))
            .arg(bbox)
            .run_expecting_failure()
            .await;
        assert!(log.contains(expected), "{log}");
    }
}

#[tokio::test]
async fn aborts_on_a_misconfigured_table() {
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
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
                typo:
                  schema: ghost_schema
                  table: table_source
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
        "},
    )
    .expect("failed to write the config");
    let output = dir.path().join("out.mbtiles");
    let log = MartinCp::generate()
        .env("RUST_LOG", "error")
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .run_expecting_failure()
        .await;
    insta::assert_snapshot!(log, @r#"
    ERROR Tile source resolution warning warning=Source typo: Unable to configure source typo because schema 'ghost_schema' does not exist. Available schemas: "Quotes' \ \' \" and Space.Dot., MixedCase, autodetect, public, schema_a, schema_b, topology
    ERROR error=TileResolutionWarningsIssued
    ERROR warnings issued during tile source resolution
    "#);
    assert!(!output.exists());
}

#[tokio::test]
async fn warns_about_a_misconfigured_table_with_on_invalid_warn() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        indoc! {"
            on_invalid: warn
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                table_source:
                  schema: public
                  table: table_source
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
                typo:
                  schema: ghost_schema
                  table: table_source
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
        "},
    )
    .expect("failed to write the config");
    let output = dir.path().join("out.mbtiles");
    let log = MartinCp::generate()
        .env("RUST_LOG", "warn")
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .arg("--max-zoom")
        .arg("1")
        .run()
        .await;
    insta::assert_snapshot!(log, @r#"
    WARN Table has no spatial index on the geometry column schema=public table=table_source geometry_column=geom
    WARN Tile source resolution warning warning=Source typo: Unable to configure source typo because schema 'ghost_schema' does not exist. Available schemas: "Quotes' \ \' \" and Space.Dot., MixedCase, autodetect, public, schema_a, schema_b, topology
    WARN Skipped features with geometry collections or empty geometries, which tiles cannot hold features.skipped=1
    "#);
    assert!(output.exists());
}

#[tokio::test]
async fn logs_the_postgres_error_when_a_scan_fails() {
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
                  srid: 4326
                  geometry_column: geom
                  geometry_type: GEOMETRY
                  filter: \"gid = 'abc'\"
        "},
    )
    .expect("failed to write the config");
    let log = MartinCp::generate()
        .env("RUST_LOG", "error")
        .with_postgres()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(dir.path().join("out.mbtiles"))
        .run_expecting_failure()
        .await;
    insta::assert_snapshot!(log, @r#"ERROR Postgres error while starting a scan: db error: ERROR: invalid input syntax for type integer: "abc""#);
}

#[cfg(unix)]
#[tokio::test]
async fn sigterm_removes_the_partial_output() {
    let dir = temp_dir();
    let output = dir.path().join("out.mbtiles");
    let log = MartinCp::generate()
        .env("RUST_LOG", "error")
        .with_postgres()
        .arg("--output-file")
        .arg(&output)
        .run_until_terminated_once_created(&output)
        .await;
    let dir_path = regex::escape(&dir.path().display().to_string());
    insta::with_settings!({ filters => vec![(dir_path.as_str(), "[DIR]")] }, {
        insta::assert_snapshot!(log, @"ERROR Interrupted; removed the partial output [DIR]/out.mbtiles");
    });
    let left: Vec<_> = fs::read_dir(dir.path())
        .expect("failed to list the output directory")
        .map(|entry| entry.expect("failed to read a directory entry").file_name())
        .collect();
    insta::assert_debug_snapshot!(left, @"[]");
}

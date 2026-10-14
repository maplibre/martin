//! The `martin generate` bulk tile generator.
#![cfg(all(feature = "test-pg", feature = "test-generate"))]

use std::fs;

use indoc::indoc;
use martin_e2e_tests::{
    Martin, MartinCp, MbtilesCli, assert_pmtiles_matches_mbtiles, metadata_listing, mlt_dump,
    temp_dir, tile_listing,
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
async fn martin_serves_the_default_pmtiles_output() {
    let dir = temp_dir();
    let pmtiles = dir.path().join("out.pmtiles");
    MartinCp::generate()
        .with_postgres()
        .arg("--source")
        .arg("points1")
        .arg("--output-file")
        .arg(&pmtiles)
        .arg("--max-zoom")
        .arg("1")
        .run()
        .await;
    let mut martin = Martin::builder()
        .arg(&pmtiles)
        .start()
        .await
        .expect("failed to start martin");
    let tile = martin.get("/out/0/0/0").await;
    insta::assert_snapshot!(tile.headers_snapshot_masking_etag(), @"
    cache-control: max-age=0, stale-while-revalidate=86400
    content-length: 193
    content-type: application/vnd.maplibre-tile
    etag: [ETAG]
    vary: accept-encoding, Origin, Access-Control-Request-Method, Access-Control-Request-Headers
    ");
    insta::assert_snapshot!(mlt_dump(&tile.mlt()), @"
    layer points1 extent 4096
      id=None geom=MULTIPOINT(4104 1845,8 1845) props=[gid=U32(Some(9))]
      id=None geom=MULTIPOINT(4145 2032,49 2032) props=[gid=U32(Some(21))]
      id=None geom=POINT(1310 3101) props=[gid=U32(Some(26))]
      id=None geom=POINT(1377 2078) props=[gid=U32(Some(27))]
      id=None geom=POINT(1415 16) props=[gid=U32(Some(16))]
      id=None geom=POINT(1455 2502) props=[gid=U32(Some(14))]
      id=None geom=POINT(166 2707) props=[gid=U32(Some(28))]
      id=None geom=POINT(1728 2658) props=[gid=U32(Some(25))]
      id=None geom=POINT(2312 2004) props=[gid=U32(Some(12))]
      id=None geom=POINT(2316 1790) props=[gid=U32(Some(24))]
      id=None geom=POINT(2337 2613) props=[gid=U32(Some(13))]
      id=None geom=POINT(2583 2400) props=[gid=U32(Some(23))]
      id=None geom=POINT(2619 1520) props=[gid=U32(Some(8))]
      id=None geom=POINT(2749 1706) props=[gid=U32(Some(20))]
      id=None geom=POINT(2754 966) props=[gid=U32(Some(17))]
      id=None geom=POINT(2780 1898) props=[gid=U32(Some(30))]
      id=None geom=POINT(2961 2263) props=[gid=U32(Some(11))]
      id=None geom=POINT(3553 2982) props=[gid=U32(Some(22))]
      id=None geom=POINT(3639 2944) props=[gid=U32(Some(10))]
      id=None geom=POINT(3673 1911) props=[gid=U32(Some(1))]
      id=None geom=POINT(3673 1911) props=[gid=U32(Some(2))]
      id=None geom=POINT(3673 1911) props=[gid=U32(Some(3))]
      id=None geom=POINT(3786 1562) props=[gid=U32(Some(6))]
      id=None geom=POINT(3802 665) props=[gid=U32(Some(7))]
      id=None geom=POINT(3825 2146) props=[gid=U32(Some(19))]
      id=None geom=POINT(3898 1322) props=[gid=U32(Some(29))]
      id=None geom=POINT(636 2785) props=[gid=U32(Some(4))]
      id=None geom=POINT(69 313) props=[gid=U32(Some(5))]
      id=None geom=POINT(769 2818) props=[gid=U32(Some(18))]
      id=None geom=POINT(862 1817) props=[gid=U32(Some(15))]
    ");
    martin.stop().await;
}

#[tokio::test]
async fn refuses_encodings_martin_cannot_serve() {
    let dir = temp_dir();
    let mut log = String::new();
    for (output, encoding) in [
        ("out.pmtiles", "gzip"),
        ("out.pmtiles", "br"),
        ("out.mbtiles", "br"),
        ("out.mbtiles", "zstd"),
    ] {
        log += &MartinCp::generate()
            .env("RUST_LOG", "error")
            .with_postgres()
            .arg("--output-file")
            .arg(dir.path().join(output))
            .arg("--encoding")
            .arg(encoding)
            .arg("--max-zoom")
            .arg("0")
            .run_expecting_failure()
            .await;
    }
    insta::assert_snapshot!(log, @"
    ERROR Martin cannot serve compressed MLT tiles from PMTiles; use `--encoding none`
    ERROR Martin cannot serve compressed MLT tiles from PMTiles; use `--encoding none`
    ERROR Martin cannot detect br compression in MBTiles; use `--encoding gzip`, `zlib` or `none`
    ERROR Martin cannot detect zstd compression in MBTiles; use `--encoding gzip`, `zlib` or `none`
    ");
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

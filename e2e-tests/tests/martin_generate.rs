//! The `martin generate` bulk tile generator.
#![cfg(all(feature = "test-pg", feature = "test-generate"))]

use std::fs;
use std::path::Path;

use martin_e2e_tests::{
    MartinCp, MbtilesCli, assert_pmtiles_matches_mbtiles, metadata_listing, temp_dir, tile_listing,
};

/// Zooms 0 to 3, uncompressed so the snapshots do not depend on the compressor.
fn generate(config: &Path, output: &Path) -> MartinCp {
    MartinCp::generate()
        .with_postgres()
        .arg("--config")
        .arg(config)
        .arg("--output-file")
        .arg(output)
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
}

#[tokio::test]
async fn generates_a_valid_tileset() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "
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
      layers:
        mixed: {}
",
    )
    .expect("failed to write the config");
    let output = dir.path().join("generated.mbtiles");
    generate(&config, &output)
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

    // The output does not depend on the thread count.
    let again = dir.path().join("again.mbtiles");
    generate(&config, &again)
        .arg("--threads")
        .arg("5")
        .run()
        .await;
    assert_eq!(tile_listing(&again).await, tile_listing(&output).await);
}

#[tokio::test]
async fn generates_pmtiles_with_the_same_tiles() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "
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
      layers:
        mixed: {}
",
    )
    .expect("failed to write the config");
    let (mbtiles, pmtiles) = (
        dir.path().join("out.mbtiles"),
        dir.path().join("out.pmtiles"),
    );
    generate(&config, &mbtiles).run().await;
    generate(&config, &pmtiles).run().await;
    assert_pmtiles_matches_mbtiles(&pmtiles, &mbtiles).await;

    // An existing archive is neither overwritten nor removed.
    let log = generate(&config, &pmtiles).run_expecting_failure().await;
    assert!(log.contains("not empty"), "{log}");
    assert_pmtiles_matches_mbtiles(&pmtiles, &mbtiles).await;
}

#[tokio::test]
async fn one_table_feeds_several_layers() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "
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
      id_column: gid
      properties:
        gid: int4
      layers:
        shapes:
          maxzoom: 1
          attributes: []
          id: drop
        points:
          minzoom: 1
          geometry: point
        lines:
          geometry: line
          buffer: 0
          simplify: 0
        detail:
          minzoom: 10
",
    )
    .expect("failed to write the config");
    let output = dir.path().join("layers.mbtiles");
    let log = generate(&config, &output).run().await;
    assert!(
        log.contains("Skipping layer `detail` of source `table_source`"),
        "{log}"
    );
    MbtilesCli::new("validate").arg(&output).run().await;
    insta::assert_snapshot!("layers_tiles", tile_listing(&output).await);
    let metadata = metadata_listing(&output).await;
    insta::with_settings!({ filters => vec![(r"martin generate v[0-9.]+[^\s]*", "martin generate v[VERSION]")] }, {
        insta::assert_snapshot!("layers_metadata", metadata);
    });
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

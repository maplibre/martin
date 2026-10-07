//! The `martin generate` bulk tile generator.
#![cfg(feature = "test-pg")]

use std::path::Path;

use martin_e2e_tests::{MartinCp, MbtilesCli, metadata_listing, temp_dir, tile_listing};

/// The arguments shared by every generation from the test database: two layers, zooms 0 to 3,
/// uncompressed so the snapshots do not depend on the compressor.
fn generate(output: &Path) -> MartinCp {
    MartinCp::generate()
        .with_postgres()
        .arg("--output-file")
        .arg(output)
        .arg("--source")
        .arg("points1")
        .arg("--source")
        .arg("table_source=mixed")
        .arg("--max-zoom")
        .arg("3")
        .arg("--encoding")
        .arg("none")
}

#[tokio::test]
async fn generates_a_valid_tileset() {
    let dir = temp_dir();
    let output = dir.path().join("generated.mbtiles");
    generate(&output).arg("--threads").arg("2").run().await;
    MbtilesCli::new("validate").arg(&output).run().await;
    insta::assert_snapshot!("tiles", tile_listing(&output).await);
    let metadata = metadata_listing(&output).await;
    insta::with_settings!({ filters => vec![(r"martin generate v[0-9.]+[^\s]*", "martin generate v[VERSION]")] }, {
        insta::assert_snapshot!("metadata", metadata);
    });

    // The output does not depend on the thread count.
    let again = dir.path().join("again.mbtiles");
    generate(&again).arg("--threads").arg("5").run().await;
    assert_eq!(tile_listing(&again).await, tile_listing(&output).await);
}

#[tokio::test]
async fn rejects_function_sources_and_pmtiles() {
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

    let log = MartinCp::generate()
        .with_postgres()
        .arg("--output-file")
        .arg(dir.path().join("out.pmtiles"))
        .run_expecting_failure()
        .await;
    assert!(log.contains("PMTiles output is not supported yet"), "{log}");
}

//! The `martin generate` bulk tile generator.
#![cfg(all(feature = "test-pg", feature = "test-generate"))]

use std::fs;
use std::path::Path;

use martin_e2e_tests::{
    MartinCp, MbtilesCli, assert_pmtiles_matches_mbtiles, metadata_listing, mlt_dump, mlt_layers,
    temp_dir, tile_listing, tiles,
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
async fn layer_expressions_filter_features_and_compute_attributes() {
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
        even:
          where: 'gid % 2 == 0'
          minzoom: 'gid > 8 ? 2 : 0'
          id: { expr: 'gid * 100' }
          attributes:
            number: gid
            size: { expr: \"gid > 5 ? 'big' : 'small'\", minzoom: 2 }
            ratio: '12 / (gid - 4)'
            source: { value: fixture }
",
    )
    .expect("failed to write the config");
    let output = dir.path().join("expressions.mbtiles");
    let log = generate(&config, &output).run().await;
    assert!(
        log.contains("Layer `even`: `12 / (gid - 4)` failed"),
        "{log}"
    );
    MbtilesCli::new("validate").arg(&output).run().await;
    let dump: Vec<String> = tiles(&output)
        .await
        .iter()
        .map(|(z, x, y, data)| format!("{z}/{x}/{y}\n{}", mlt_dump(&mlt_layers(data))))
        .collect();
    insta::assert_snapshot!("expressions_tiles", dump.join("\n"));
    let metadata = metadata_listing(&output).await;
    insta::with_settings!({ filters => vec![(r"martin generate v[0-9.]+[^\s]*", "martin generate v[VERSION]")] }, {
        insta::assert_snapshot!("expressions_metadata", metadata);
    });
}

#[tokio::test]
async fn layer_rules_override_settings_per_feature() {
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
        ranked:
          maxzoom: 2
          attributes:
            number: gid
            band: { value: low }
          rules:
            - where: 'gid > 15'
              attributes:
                band: { value: high }
                half: { expr: 'gid / 2', minzoom: 2 }
            - where: 'gid > 5'
              minzoom: 1
              simplify: 0
              attributes:
                band: { value: mid }
            - minzoom: 'gid % 2 == 0 ? 1 : 2'
",
    )
    .expect("failed to write the config");
    let output = dir.path().join("rules.mbtiles");
    generate(&config, &output).run().await;
    MbtilesCli::new("validate").arg(&output).run().await;
    let dump: Vec<String> = tiles(&output)
        .await
        .iter()
        .map(|(z, x, y, data)| format!("{z}/{x}/{y}\n{}", mlt_dump(&mlt_layers(data))))
        .collect();
    insta::assert_snapshot!("rules_tiles", dump.join("\n"));
    let metadata = metadata_listing(&output).await;
    insta::with_settings!({ filters => vec![(r"martin generate v[0-9.]+[^\s]*", "martin generate v[VERSION]")] }, {
        insta::assert_snapshot!("rules_metadata", metadata);
    });
}

#[tokio::test]
async fn sort_by_sets_the_draw_order() {
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
        sorted:
          maxzoom: 1
          attributes:
            number: gid
          sort_by: [{ expr: 'gid % 4', desc: true }, gid]
",
    )
    .expect("failed to write the config");
    let output = dir.path().join("sorted.mbtiles");
    generate(&config, &output).run().await;
    MbtilesCli::new("validate").arg(&output).run().await;
    let order: Vec<String> = tiles(&output)
        .await
        .iter()
        .flat_map(|(z, x, y, data)| {
            mlt_layers(data).into_iter().map(move |layer| {
                let ids: Vec<String> = layer
                    .features()
                    .iter()
                    .map(|feature| feature.id().map_or("-".to_owned(), |id| id.to_string()))
                    .collect();
                format!("{z}/{x}/{y} {} {}", layer.name(), ids.join(" "))
            })
        })
        .collect();
    insta::assert_snapshot!("sort_by_order", order.join("\n"));
    let metadata = metadata_listing(&output).await;
    insta::with_settings!({ filters => vec![(r"martin generate v[0-9.]+[^\s]*", "martin generate v[VERSION]")] }, {
        insta::assert_snapshot!("sort_by_metadata", metadata);
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

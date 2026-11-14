//! `martin cp --mode bulk` reading an OpenStreetMap PBF file from the config's `osm` section.

use std::fs;

use indoc::formatdoc;
use martin_e2e_tests::{MartinCp, MbtilesCli, fixture, mvt_dump, temp_dir, tile_listing, tiles};

#[tokio::test]
async fn monaco_renders_roads_buildings_cafes_and_water() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        formatdoc! {"
            osm:
              sources:
                monaco:
                  path: {path}
                  tags: [highway, building, amenity, name, natural]
                  layers:
                    roads:
                      geometry: line
                      where: \"highway in ['primary', 'secondary', 'tertiary']\"
                      attributes: [highway]
                    buildings:
                      geometry: polygon
                      minzoom: 13
                      where: \"building != null\"
                      attributes: []
                    cafes:
                      geometry: point
                      where: \"amenity == 'cafe' && name != null\"
                      attributes: [name]
                    water:
                      geometry: polygon
                      where: \"natural == 'water' || osm_type == 'relation' && natural != null\"
                      attributes: [osm_type, natural]
        ", path = fixture("osm/monaco.osm.pbf").display()},
    )
    .expect("failed to write the config");
    let output = dir.path().join("monaco.mbtiles");
    MartinCp::bulk()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .arg("--format")
        .arg("mvt")
        .arg("--encoding")
        .arg("none")
        .arg("--min-zoom")
        .arg("10")
        .arg("--max-zoom")
        .arg("13")
        .arg("--threads")
        .arg("2")
        .run()
        .await;
    MbtilesCli::new("validate").arg(&output).run().await;
    insta::assert_snapshot!("tiles", tile_listing(&output).await);
    let tiles = tiles(&output).await;
    insta::assert_snapshot!("z10", mvt_dump(&tiles[0].3));
}

#[tokio::test]
async fn the_tileset_does_not_depend_on_the_thread_count() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        formatdoc! {"
            osm:
              sources:
                monaco:
                  path: {path}
                  tags: [highway, building, name]
                  layers:
                    roads:
                      geometry: line
                      where: \"highway != null\"
                      attributes: [highway, name]
                    buildings:
                      geometry: polygon
                      where: \"building != null\"
                      attributes: []
        ", path = fixture("osm/monaco.osm.pbf").display()},
    )
    .expect("failed to write the config");
    let mut runs = Vec::new();
    for threads in ["1", "6"] {
        let output = dir.path().join(format!("threads-{threads}.mbtiles"));
        MartinCp::bulk()
            .arg("--config")
            .arg(&config)
            .arg("--output-file")
            .arg(&output)
            .arg("--max-zoom")
            .arg("14")
            .arg("--threads")
            .arg(threads)
            .run()
            .await;
        runs.push(tiles(&output).await);
    }
    assert_eq!(runs[0], runs[1]);
}

#[tokio::test]
async fn osm_and_database_sources_in_one_run_are_refused() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        formatdoc! {"
            postgres:
              connection_string: postgres://localhost:1/unreachable
            osm:
              sources:
                monaco:
                  path: {path}
                  layers:
                    everything: {{}}
        ", path = fixture("osm/monaco.osm.pbf").display()},
    )
    .expect("failed to write the config");
    let stderr = MartinCp::bulk()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(dir.path().join("out.mbtiles"))
        .run_expecting_failure()
        .await;
    insta::assert_snapshot!(stderr.lines().last().unwrap_or_default(), @"ERROR `--mode bulk` renders OSM sources or database sources in one run, not both; pick them with `--source`");
}

#[tokio::test]
async fn a_file_with_node_ids_out_of_order_is_refused() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    let path = fixture("osm/unsorted.osm.pbf");
    fs::write(
        &config,
        formatdoc! {"
            osm:
              sources:
                unsorted:
                  path: {path}
                  layers:
                    everything: {{}}
        ", path = path.display()},
    )
    .expect("failed to write the config");
    let output = dir.path().join("out.mbtiles");
    let stderr = MartinCp::bulk()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .run_expecting_failure()
        .await;
    let last = stderr.lines().last().unwrap_or_default();
    insta::assert_snapshot!(last.replace(&path.display().to_string(), "unsorted.osm.pbf"), @"ERROR Cannot read unsorted.osm.pbf: node IDs must be loaded in ascending order: 1 after 2");
    assert!(!output.exists());
}

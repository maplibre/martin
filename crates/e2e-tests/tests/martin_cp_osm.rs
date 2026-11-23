//! `martin cp --mode bulk` reading an OpenStreetMap PBF file from the config's `osm` section.

use std::fs;

use indoc::formatdoc;
use martin_e2e_tests::{MartinCp, MbtilesCli, fixture, metadata, mvt_dump, temp_dir, tile_listing, tiles};

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
async fn a_hidden_attribute_keys_thinning_but_no_tile_or_metadata_carries_it() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        formatdoc! {"
            osm:
              sources:
                monaco:
                  path: {path}
                  tags: [\"addr:housenumber\", \"addr:street\"]
                  layers:
                    housenumber:
                      geometry: point
                      where: \"feature['addr:housenumber'] != null\"
                      attributes:
                        housenumber: \"feature['addr:housenumber']\"
                        _partition:
                          expr: \"feature['addr:street'] != null ? feature['addr:street'] + ' ' + feature['addr:housenumber'] : null\"
                          hidden: true
                      thinning: {{ method: tile, keys: [_partition] }}
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
        .arg("--mvt-quirk")
        .arg("keep-unused-keys")
        .arg("--min-zoom")
        .arg("14")
        .arg("--max-zoom")
        .arg("14")
        .run()
        .await;
    let carrying: Vec<_> = tiles(&output)
        .await
        .into_iter()
        .filter(|tile| tile.3.windows(b"_partition".len()).any(|bytes| bytes == b"_partition"))
        .map(|(z, x, y, _)| (z, x, y))
        .collect();
    assert_eq!(carrying, []);
    insta::assert_snapshot!(metadata(&output).await["json"], @r#"{"vector_layers":[{"id":"housenumber","fields":{"housenumber":"String"},"maxzoom":14,"minzoom":14}]}"#);
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
    insta::assert_snapshot!(stderr.lines().last().unwrap_or_default(), @"ERROR `--mode bulk` renders file sources (`osm`, `shapefile`) or database sources in one run, not both; pick them with `--source`");
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

#[tokio::test]
async fn encodes_with_the_configured_convert_to_mlt_settings() {
    let dir = temp_dir();
    let default_config = dir.path().join("default.yaml");
    fs::write(
        &default_config,
        formatdoc! {"
            osm:
              sources:
                monaco:
                  path: {path}
                  tags: [building]
                  layers:
                    buildings:
                      geometry: polygon
                      where: \"building != null\"
                      attributes: []
        ", path = fixture("osm/monaco.osm.pbf").display()},
    )
    .expect("failed to write the config");
    let tessellated_config = dir.path().join("tessellated.yaml");
    fs::write(
        &tessellated_config,
        formatdoc! {"
            convert_to_mlt:
              tessellate: true
            osm:
              sources:
                monaco:
                  path: {path}
                  tags: [building]
                  layers:
                    buildings:
                      geometry: polygon
                      where: \"building != null\"
                      attributes: []
        ", path = fixture("osm/monaco.osm.pbf").display()},
    )
    .expect("failed to write the config");
    let default_output = dir.path().join("default.mbtiles");
    MartinCp::bulk()
        .arg("--config")
        .arg(&default_config)
        .arg("--output-file")
        .arg(&default_output)
        .arg("--encoding")
        .arg("none")
        .arg("--min-zoom")
        .arg("12")
        .arg("--max-zoom")
        .arg("12")
        .run()
        .await;
    let tessellated_output = dir.path().join("tessellated.mbtiles");
    MartinCp::bulk()
        .arg("--config")
        .arg(&tessellated_config)
        .arg("--output-file")
        .arg(&tessellated_output)
        .arg("--encoding")
        .arg("none")
        .arg("--min-zoom")
        .arg("12")
        .arg("--max-zoom")
        .arg("12")
        .run()
        .await;
    let listings = format!(
        "default:\n{}\ntessellated:\n{}",
        tile_listing(&default_output).await,
        tile_listing(&tessellated_output).await
    );
    insta::assert_snapshot!(listings);
}

#[tokio::test]
async fn refuses_mlt_output_with_mlt_conversion_disabled() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        formatdoc! {"
            convert_to_mlt: disabled
            osm:
              sources:
                monaco:
                  path: {path}
                  layers:
                    everything: {{}}
        ", path = fixture("osm/monaco.osm.pbf").display()},
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
    insta::assert_snapshot!(stderr.lines().last().unwrap_or_default(), @"ERROR Source `monaco` sets `convert_to_mlt: disabled`, but `--mode bulk` writes MLT unless given `--format mvt`");
    assert!(!output.exists());
}

#[tokio::test]
async fn churches_and_cafes_write_into_one_pois_layer_churches_first() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        formatdoc! {"
            osm:
              sources:
                monaco:
                  path: {path}
                  tags: [building, amenity, name]
                  layers:
                    churches:
                      layer: pois
                      geometry: polygon
                      where: \"building == 'church' && name != null\"
                      attributes: [name]
                    cafes:
                      layer: pois
                      geometry: point
                      where: \"amenity == 'cafe' && name != null\"
                      attributes: [name]
        ", path = fixture("osm/monaco.osm.pbf").display()},
    )
    .expect("failed to write the config");
    let output = dir.path().join("pois.mbtiles");
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
        .arg("14")
        .arg("--max-zoom")
        .arg("14")
        .run()
        .await;
    let tiles = tiles(&output).await;
    let tile = tiles
        .iter()
        .find(|(z, x, y, _)| (*z, *x, *y) == (14, 8529, 10410))
        .expect("tile 14/8529/10410 was written");
    insta::assert_snapshot!(mvt_dump(&tile.3), @r#"
    layer: 0
      name: pois
      version: 2
      extent: 4096
      feature: 0
        id: 943994592
        geometry: RING[count=11](4176 3272,4153 3257,4155 3252,4142 3244,4145 3234,4143 3224,4147 3215,4157 3214,4159 3209,4166 3205,4176 3272)[OUTER]
        properties:
          name = "Église Saint-Charles"
      feature: 1
        id: 944001042
        geometry: RING[count=7](3089 3900,3122 3932,3076 3979,3057 3960,3053 3964,3039 3952,3089 3900)[OUTER]
        properties:
          name = "Église des Franciscains"
      feature: 2
        id: 30874685431
        geometry: POINT(3721,4136)
        properties:
          name = "Le Palace Café"
      feature: 3
        id: 30874686101
        geometry: POINT(3433,4031)
        properties:
          name = "Richmond Bar"
      feature: 4
        id: 78221436871
        geometry: POINT(4061,3348)
        properties:
          name = "Costa"
    "#);
}

#[tokio::test]
async fn a_layer_written_by_several_layers_does_not_depend_on_the_thread_count() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        formatdoc! {"
            osm:
              sources:
                monaco:
                  path: {path}
                  tags: [highway, building, amenity, name]
                  layers:
                    roads:
                      layer: everything
                      geometry: line
                      where: \"highway != null\"
                      attributes: [highway, name]
                    buildings:
                      layer: everything
                      geometry: polygon
                      where: \"building != null\"
                      attributes: [building]
                    labels:
                      layer: everything
                      geometry: polygon_point_on_surface
                      where: \"building != null && name != null\"
                      attributes: [name]
                    amenities:
                      layer: everything
                      geometry: point
                      where: \"amenity != null\"
                      attributes: [amenity]
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
async fn layers_writing_into_one_layer_with_different_sort_by_are_refused() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        formatdoc! {"
            osm:
              sources:
                monaco:
                  path: {path}
                  tags: [building, amenity, name]
                  layers:
                    buildings:
                      layer: pois
                      geometry: polygon
                      where: \"building != null\"
                      sort_by: [name]
                    cafes:
                      layer: pois
                      geometry: point
                      where: \"amenity == 'cafe'\"
                      sort_by: [amenity]
        ", path = fixture("osm/monaco.osm.pbf").display()},
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
    insta::assert_snapshot!(stderr.lines().last().unwrap_or_default(), @"ERROR layers `buildings` of source `monaco` and `cafes` of source `monaco` both write into layer `pois` but set `sort_by` differently; set it equal on both, or on only one of them");
    assert!(!output.exists());
}

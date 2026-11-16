//! `martin cp --mode bulk` reading an ESRI shapefile from the config's `shapefile` section.

use std::fs;

use indoc::formatdoc;
use martin_e2e_tests::{MartinCp, MbtilesCli, fixture, mvt_dump, temp_dir, tile_listing, tiles};

#[tokio::test]
async fn a_zipped_ocean_shapefile_and_monaco_render_into_one_tileset() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        formatdoc! {"
            shapefile:
              sources:
                ocean:
                  path: {ocean}
                  layers:
                    ocean:
                      geometry: polygon
                      attributes: []
            osm:
              sources:
                monaco:
                  path: {monaco}
                  tags: [amenity, name]
                  layers:
                    cafes:
                      geometry: point
                      where: \"amenity == 'cafe' && name != null\"
                      attributes: [name]
        ", ocean = fixture("shapefile/ocean.zip").display(), monaco = fixture("osm/monaco.osm.pbf").display()},
    )
    .expect("failed to write the config");
    let output = dir.path().join("world.mbtiles");
    MartinCp::bulk()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(&output)
        .arg("--format")
        .arg("mvt")
        .arg("--encoding")
        .arg("none")
        .arg("--max-zoom")
        .arg("1")
        .arg("--threads")
        .arg("2")
        .arg("--temp-dir")
        .arg(dir.path())
        .run()
        .await;
    MbtilesCli::new("validate").arg(&output).run().await;
    insta::assert_snapshot!("tiles", tile_listing(&output).await);
    let tiles = tiles(&output).await;
    insta::assert_snapshot!("z0", mvt_dump(&tiles[0].3));
}

#[tokio::test]
async fn a_source_id_in_shapefile_and_osm_is_refused() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        formatdoc! {"
            shapefile:
              sources:
                water:
                  path: {shapefile}
                  layers:
                    ocean: {{}}
            osm:
              sources:
                water:
                  path: {osm}
                  tags: [natural]
                  layers:
                    lakes: {{}}
        ", shapefile = fixture("shapefile/ocean.zip").display(), osm = fixture("osm/monaco.osm.pbf").display()},
    )
    .expect("failed to write the config");
    let stderr = MartinCp::bulk()
        .arg("--config")
        .arg(&config)
        .arg("--output-file")
        .arg(dir.path().join("out.mbtiles"))
        .run_expecting_failure()
        .await;
    insta::assert_snapshot!(stderr.lines().last().unwrap_or_default(), @"ERROR Source id `water` is both a `shapefile` and an `osm` source; give them different ids");
}

#[tokio::test]
async fn shapefile_and_database_sources_in_one_run_are_refused() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        formatdoc! {"
            postgres:
              connection_string: postgres://localhost:1/unreachable
            shapefile:
              sources:
                ocean:
                  path: {path}
                  layers:
                    ocean: {{}}
        ", path = fixture("shapefile/ocean.zip").display()},
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

//! `martin test` on an OpenStreetMap PBF file from the config's `osm` section.

use std::fs;

use indoc::formatdoc;
use martin_e2e_tests::{MartinTest, fixture, temp_dir};

#[tokio::test]
async fn generate_then_run_passes_every_case_of_a_monaco_spec() {
    let dir = temp_dir();
    let config = dir.path().join("config.yaml");
    let spec = dir.path().join("monaco.spec.yaml");
    fs::write(
        &config,
        formatdoc! {"
            osm:
              sources:
                monaco:
                  path: {path}
                  tags: [highway, name]
                  layers:
                    roads:
                      geometry: line
                      minzoom: \"highway == 'primary' ? 10 : 14\"
                      where: \"highway in ['primary', 'residential']\"
                      attributes: [highway, name]
        ", path = fixture("osm/monaco.osm.pbf").display()},
    )
    .expect("failed to write the config");
    let (generated, _) = MartinTest::new("generate")
        .arg("--config")
        .arg(&config)
        .arg("--output")
        .arg(&spec)
        .arg("--accept")
        .run()
        .await;
    let (ran, log) = MartinTest::new("run")
        .arg("--config")
        .arg(&config)
        .arg(&spec)
        .run()
        .await;
    let summary = log.lines().find(|line| line.contains("Summary")).unwrap_or_default();
    let summary = summary.split_once(']').map_or(summary, |(_, rest)| rest).trim();
    insta::assert_snapshot!(format!("{:?} {:?} {summary}", generated.code(), ran.code()), @"Some(0) Some(0) 5 cases run: 5 passed");
}

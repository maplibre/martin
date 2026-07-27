//! Configuration file keys martin does not recognize.

use std::fs;

use martin_integration_tests::{Martin, MartinBuilder};
use tempfile::TempDir;

const PMTILES_SOURCE: &str = "
pmtiles:
  sources:
    pmt: tests/fixtures/pmtiles/png.pmtiles
";

/// Write `yaml` into a fresh temp dir and start `builder` on it.
/// The dir is returned because it has to outlive the server.
/// Relative paths in `yaml` resolve against the workspace root, martin's working directory here.
async fn start_with_config(builder: MartinBuilder, yaml: &str) -> (TempDir, Martin) {
    let dir = tempfile::tempdir().expect("failed to create a temp dir");
    let path = dir.path().join("config.yaml");
    fs::write(&path, yaml).expect("failed to write the config file");
    let martin = builder
        .arg("--config")
        .arg(&path)
        .start()
        .await
        .expect("failed to start martin");
    (dir, martin)
}

/// The keys named by every `Ignoring unrecognized configuration key '<key>'` line, sorted.
/// The lines are consumed, so [`Martin::assert_log_clean`] no longer sees them.
fn unrecognized_keys(martin: &mut Martin) -> Vec<String> {
    let mut keys = martin
        .take_log_lines("Ignoring unrecognized configuration key")
        .iter()
        .map(|line| {
            line.split('\'')
                .nth(1)
                .expect("the warning quotes the key it ignored")
                .to_owned()
        })
        .collect::<Vec<_>>();
    keys.sort();
    keys
}

#[tokio::test]
async fn every_section_reports_its_unrecognized_keys_by_full_path() {
    let (_dir, mut martin) = start_with_config(
        Martin::builder(),
        "
warning: 'unrecognized'
observability:
  warning: 'unrecognized'
  metrics:
    warning: 'unrecognized'
cors:
  warning: 'unrecognized'
  origin: ['*']
pmtiles:
  warning: 'unrecognized'
  sources:
    pmt: tests/fixtures/pmtiles/png.pmtiles
mbtiles:
  warning: 'unrecognized'
geojson:
  warning: 'unrecognized'
sprites:
  warning: 'unrecognized'
  paths: tests/fixtures/sprites/src1
styles:
  warning: 'unrecognized'
  sources:
    maplibre: tests/fixtures/styles/maplibre_demo.json
fonts:
  warning: 'unrecognized'
",
    )
    .await;

    martin.stop().await;
    insta::assert_snapshot!(unrecognized_keys(&mut martin).join("\n"), @r"
    cors.warning
    fonts.warning
    geojson.warning
    mbtiles.warning
    observability.metrics.warning
    observability.warning
    pmtiles.warning
    sprites.warning
    styles.warning
    warning
    ");
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn an_unrecognized_key_does_not_stop_the_configured_sources_from_being_served() {
    let (_dir, mut martin) = start_with_config(
        Martin::builder(),
        "
warning: 'unrecognized'
pmtiles:
  warning: 'unrecognized'
  sources:
    pmt: tests/fixtures/pmtiles/png.pmtiles
",
    )
    .await;

    assert_eq!(martin.get("/health").await.status(), 200);
    assert!(
        martin.get("/catalog").await.json()["tiles"]
            .get("pmt")
            .is_some(),
        "the source next to the unrecognized keys must still be published"
    );
    let tile = martin.get("/pmt/0/0/0").await;
    assert_eq!(tile.status(), 200);
    assert!(
        tile.body().starts_with(b"\x89PNG"),
        "expected a png tile body"
    );

    martin.stop().await;
    assert_eq!(
        unrecognized_keys(&mut martin),
        ["pmtiles.warning", "warning"]
    );
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn the_cog_section_is_reported_in_builds_with_and_without_unstable_cog() {
    let (_dir, mut martin) = start_with_config(
        Martin::builder(),
        &format!(
            "
cog:
  warning: 'unrecognized'
{PMTILES_SOURCE}"
        ),
    )
    .await;

    martin.stop().await;
    let keys = unrecognized_keys(&mut martin);
    assert!(
        keys == ["cog"] || keys == ["cog.warning"],
        "expected the whole section without unstable-cog, or just the key with it, got {keys:?}"
    );
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn an_object_store_option_is_reported_when_its_value_is_not_a_scalar() {
    let (_dir, mut martin) = start_with_config(
        Martin::builder(),
        "
pmtiles:
  connect_timeout: 5s
  timeout:
    nested: 1
  sources:
    pmt: tests/fixtures/pmtiles/png.pmtiles
",
    )
    .await;

    martin.stop().await;
    martin.assert_log_contains(
        r#"Ignoring unrecognized configuration key 'pmtiles.timeout': Object {"nested": Number(1)}. Only boolean, string or number values are allowed here."#,
    );
    assert_eq!(
        unrecognized_keys(&mut martin),
        Vec::<String>::new(),
        "`connect_timeout` names the same object store option set, so its scalar value is accepted"
    );
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn unrecognized_keys_are_left_out_of_the_saved_config() {
    let dir = tempfile::tempdir().expect("failed to create a temp dir");
    let save_config = dir.path().join("save_config.yaml");
    let (_config_dir, mut martin) = start_with_config(
        Martin::builder().arg("--save-config").arg(&save_config),
        "
warning: 'unrecognized'
pmtiles:
  warning: 'unrecognized'
  sources:
    pmt: tests/fixtures/pmtiles/png.pmtiles
",
    )
    .await;

    let saved = fs::read_to_string(&save_config).expect("martin did not write --save-config");
    insta::assert_snapshot!(saved, @r"
    listen_addresses: 127.0.0.1:0
    pmtiles:
      sources:
        pmt: tests/fixtures/pmtiles/png.pmtiles
    ");

    martin.stop().await;
    assert_eq!(
        unrecognized_keys(&mut martin),
        ["pmtiles.warning", "warning"]
    );
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[cfg(feature = "test-pg")]
#[tokio::test]
async fn the_postgres_section_reports_its_unrecognized_keys_per_source() {
    let (_dir, mut martin) = start_with_config(
        Martin::builder().with_postgres(),
        "
postgres:
  warning: 'unrecognized'
  connection_string: ${DATABASE_URL}
  pool_size: 1
  tables:
    points1:
      warning: 'unrecognized'
      schema: public
      table: points1
      srid: 4326
      geometry_column: geom
      geometry_type: POINT
      properties:
        gid: int4
  functions:
    function_zxy_query:
      warning: 'unrecognized'
      schema: public
      function: function_zxy_query
",
    )
    .await;

    martin.stop().await;
    insta::assert_snapshot!(unrecognized_keys(&mut martin).join("\n"), @r"
    postgres.functions.function_zxy_query.warning
    postgres.tables.points1.warning
    postgres.warning
    ");
    martin.assert_log_clean();
}

#[cfg(feature = "test-pg")]
#[tokio::test]
async fn the_postgres_auto_publish_section_reports_its_unrecognized_keys() {
    let (_dir, mut martin) = start_with_config(
        Martin::builder().with_postgres(),
        "
postgres:
  connection_string: ${DATABASE_URL}
  pool_size: 1
  auto_publish:
    warning: 'unrecognized'
    tables:
      warning: 'unrecognized'
      from_schemas: MixedCase
    functions:
      warning: 'unrecognized'
      from_schemas: MixedCase
",
    )
    .await;

    martin.stop().await;
    insta::assert_snapshot!(unrecognized_keys(&mut martin).join("\n"), @r"
    postgres.auto_publish.functions.warning
    postgres.auto_publish.tables.warning
    postgres.auto_publish.warning
    ");
    for unindexed in [
        "Table public.mat_view has no spatial index on column geom",
        "Table public.table_source has no spatial index on column geom",
        "Table public.table_source_geog has no spatial index on column geog",
    ] {
        martin.assert_log_contains(unindexed);
    }
    martin.assert_log_clean();
}

//! `summary`, `meta-all`, `meta-get` and `validate` in the `mbtiles` CLI.

use std::path::{Path, PathBuf};

use approx::assert_relative_eq;
use martin_e2e_tests::{
    MbtilesCli, fixture, mbtiles_fixture, mbtiles_from_sql, metadata, summary, summary_filters,
    temp_dir,
};
use regex::Regex;
use rstest::rstest;
use tempfile::TempDir;

/// The hash `bad_hash.mbtiles` claims in its metadata, and the one its tiles actually add up to.
const STALE_AGG_HASH: &str = "CAFEC0DEDEADBEEFDEADBEEFDEADBEEF";
const REAL_AGG_HASH: &str = "E89600605FA137D684A10EE91463CEE0";

/// Build one of the deliberately broken fixtures from `tests/fixtures/files` into `dir`.
async fn broken_fixture(dir: &Path, name: &str) -> PathBuf {
    let dest = dir.join(format!("{name}.mbtiles"));
    mbtiles_from_sql(fixture(&format!("files/{name}.sql")), &dest).await;
    dest
}

/// A run's output with the parts that are not the same on every machine replaced, so that it can
/// be snapshotted: this run's temp directory, and the page geometry, which depends on the sqlite
/// build that wrote the file.
fn redact(dir: &TempDir, output: &str) -> String {
    let output = output.replace(&dir.path().display().to_string(), "[TMP]");
    let output = output.replace(std::path::MAIN_SEPARATOR, "/");
    Regex::new(r"(?m)^(File size:|SQL page size:|SQL page count:)(\s+).*$")
        .expect("the pattern is valid")
        .replace_all(&output, "$1$2[SQLITE]")
        .into_owned()
}

#[tokio::test]
async fn summary_reports_the_schema_and_per_zoom_statistics() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;

    let summary = summary(&source).run_json().await;

    // Zoom 2 is the only level holding more than one tile, so it is the only place where the
    // smallest, largest and average sizes differ from each other.
    insta::with_settings!({filters => summary_filters()}, {
        insta::assert_json_snapshot!(summary);
    });
}

#[tokio::test]
async fn summary_reports_the_whole_world_as_the_bounding_box() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;

    let summary = summary(&source).run_json().await;

    let bbox: Vec<f64> = summary["bbox"]
        .as_array()
        .expect("the summary reports a bounding box")
        .iter()
        .map(|value| value.as_f64().expect("a coordinate is a number"))
        .collect();
    // The tile at 0/0/0 covers the whole web-mercator extent, and the reported box is
    // computed by trigonometry, so it lands a few ULPs off the round numbers.
    assert_relative_eq!(bbox[0], -180.0);
    assert_relative_eq!(bbox[1], -85.051_128_779_806_6);
    assert_relative_eq!(bbox[2], 180.0);
    assert_relative_eq!(bbox[3], 85.051_128_779_806_6);
}

#[tokio::test]
async fn summary_reports_how_the_file_is_laid_out_on_disk() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;

    let summary = summary(&source).run_json().await;

    let file_size = summary["file_size"]
        .as_u64()
        .expect("the summary reports a file size");
    let page_size = summary["page_size"]
        .as_u64()
        .expect("the summary reports a page size");
    let page_count = summary["page_count"]
        .as_u64()
        .expect("the summary reports a page count");
    assert!(page_size > 0 && page_count > 0);
    assert_eq!(
        page_size * page_count,
        file_size,
        "an mbtiles file is a whole number of sqlite pages"
    );
}

#[tokio::test]
async fn the_two_json_summary_formats_carry_the_same_document() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;

    let compact = summary(&source).run().await;
    let pretty = MbtilesCli::new("summary")
        .arg("--format")
        .arg("json-pretty")
        .arg(&source)
        .run()
        .await;

    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&compact).expect("`--format json` prints json"),
        serde_json::from_str::<serde_json::Value>(&pretty)
            .expect("`--format json-pretty` prints json"),
    );
    assert_eq!(
        compact.trim_end().lines().count(),
        1,
        "`--format json` prints one line:\n{compact}"
    );
    assert!(
        pretty.lines().count() > 1,
        "`--format json-pretty` prints an indented document:\n{pretty}"
    );
}

#[tokio::test]
async fn summary_prints_a_table_by_default() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;

    let output = MbtilesCli::new("summary").arg(&source).run().await;

    insta::assert_snapshot!(redact(&dir, &output));
}

#[tokio::test]
async fn meta_all_prints_every_metadata_row() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;

    let output = MbtilesCli::new("meta-all").arg(&source).run().await;

    // Every row of the metadata table appears, and `json` is unpacked into the tilejson block
    // rather than printed verbatim.
    insta::assert_snapshot!(redact(&dir, &output));
}

#[tokio::test]
async fn meta_get_reads_one_value() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;

    let output = MbtilesCli::new("meta-get")
        .arg(&source)
        .arg("name")
        .run()
        .await;

    insta::assert_snapshot!(output, @"Major cities from Natural Earth data");
}

#[tokio::test]
async fn meta_get_says_nothing_about_a_key_that_is_not_there() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;

    let output = MbtilesCli::new("meta-get")
        .arg(&source)
        .arg("missing_value")
        .run()
        .await;

    insta::assert_snapshot!(output, @"");
}

#[tokio::test]
async fn meta_all_documents_itself() {
    let help = MbtilesCli::new("meta-all").arg("--help").run().await;

    insta::assert_snapshot!(help, @"
    Prints all values in the metadata table in a free-style, unstable YAML format

    Usage: mbtiles meta-all <FILE>

    Arguments:
      <FILE>  MBTiles file to read from

    Options:
      -h, --help  Print help
    ");
}

#[tokio::test]
async fn meta_get_documents_itself() {
    let help = MbtilesCli::new("meta-get").arg("--help").run().await;

    insta::assert_snapshot!(help, @"
    Gets a single value from the MBTiles metadata table

    Usage: mbtiles meta-get <FILE> <KEY>

    Arguments:
      <FILE>  MBTiles file to read a value from
      <KEY>   Value to read

    Options:
      -h, --help  Print help
    ");
}

#[rstest]
#[case::flat_with_hash("zoomed_world_cities")]
#[case::normalized_with_duplicate_ids("normalized-dedup-id")]
#[tokio::test]
async fn validate_accepts_a_file_whose_hashes_match(#[case] name: &str) {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), name).await;

    let output = MbtilesCli::new("validate").arg(&source).run().await;

    // Named per case: rstest runs both through this one function, so the snapshots cannot be
    // told apart by the function name alone.
    insta::assert_snapshot!(format!("validate_accepts_{name}"), redact(&dir, &output));
}

#[tokio::test]
async fn validate_rejects_a_tile_outside_its_zoom_level() {
    let dir = temp_dir();
    let source = broken_fixture(dir.path(), "invalid-tile-idx").await;

    let output = MbtilesCli::new("validate").arg(&source).run_failing().await;

    insta::assert_snapshot!(redact(&dir, &output), @"
     INFO Integrity check passed mbtiles.file=[TMP]/invalid-tile-idx.mbtiles integrity_check=Quick
    ERROR At least one tile in the tiles table/view has an invalid value: zoom_level=6, tile_column=10, tile_row=64 in MBTile file [TMP]/invalid-tile-idx.mbtiles
    ");
}

#[tokio::test]
async fn validate_rejects_a_stale_aggregate_hash() {
    let dir = temp_dir();
    let source = broken_fixture(dir.path(), "bad_hash").await;

    let output = MbtilesCli::new("validate").arg(&source).run_failing().await;

    // The per-tile hashes are fine here; only the file-wide roll-up is wrong.
    insta::assert_snapshot!(redact(&dir, &output), @"
     INFO Integrity check passed mbtiles.file=[TMP]/bad_hash.mbtiles integrity_check=Quick
     INFO All values in the `tiles` table/view are valid mbtiles.file=[TMP]/bad_hash.mbtiles
     INFO All tile hashes are valid mbtiles.file=[TMP]/bad_hash.mbtiles
    ERROR Computed aggregate tiles hash E89600605FA137D684A10EE91463CEE0 does not match tile data in metadata CAFEC0DEDEADBEEFDEADBEEFDEADBEEF for MBTile file [TMP]/bad_hash.mbtiles
    ");
}

#[tokio::test]
async fn validate_agg_hash_update_repairs_a_stale_hash() {
    let dir = temp_dir();
    let source = broken_fixture(dir.path(), "bad_hash").await;
    assert_eq!(metadata(&source).await["agg_tiles_hash"], STALE_AGG_HASH);

    let output = MbtilesCli::new("validate")
        .arg("--agg-hash")
        .arg("update")
        .arg(&source)
        .run()
        .await;

    insta::assert_snapshot!(redact(&dir, &output), @"
    INFO Integrity check passed mbtiles.file=[TMP]/bad_hash.mbtiles integrity_check=Quick
    INFO All values in the `tiles` table/view are valid mbtiles.file=[TMP]/bad_hash.mbtiles
    INFO All tile hashes are valid mbtiles.file=[TMP]/bad_hash.mbtiles
    INFO Updating agg_tiles_hash mbtiles.file=[TMP]/bad_hash.mbtiles agg_tiles_hash.old=CAFEC0DEDEADBEEFDEADBEEFDEADBEEF agg_tiles_hash.new=E89600605FA137D684A10EE91463CEE0
    ");
    assert_eq!(metadata(&source).await["agg_tiles_hash"], REAL_AGG_HASH);

    let output = MbtilesCli::new("validate").arg(&source).run().await;
    insta::assert_snapshot!("validate_after_the_repair", redact(&dir, &output));
}

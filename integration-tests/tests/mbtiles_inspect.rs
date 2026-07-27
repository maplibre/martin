//! `summary`, `meta-all`, `meta-get` and `validate` in the `mbtiles` CLI.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use approx::assert_relative_eq;
use martin_integration_tests::{MbtilesCli, fixture, mbtiles_fixture, mbtiles_from_sql};
use rstest::rstest;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection as _, SqliteConnection};
use tempfile::TempDir;

/// The hash `bad_hash.mbtiles` claims in its metadata, and the one its tiles actually add up to.
const STALE_AGG_HASH: &str = "CAFEC0DEDEADBEEFDEADBEEFDEADBEEF";
const REAL_AGG_HASH: &str = "E89600605FA137D684A10EE91463CEE0";

fn temp_dir() -> TempDir {
    tempfile::tempdir().expect("failed to create a temp dir")
}

/// Build one of the deliberately broken fixtures from `tests/fixtures/files` into `dir`.
async fn broken_fixture(dir: &Path, name: &str) -> PathBuf {
    let dest = dir.join(format!("{name}.mbtiles"));
    mbtiles_from_sql(fixture(&format!("files/{name}.sql")), &dest).await;
    dest
}

/// Every row of the `metadata` table, read straight from the file.
async fn metadata(path: &Path) -> BTreeMap<String, String> {
    let options = SqliteConnectOptions::new().filename(path).read_only(true);
    let mut conn = SqliteConnection::connect_with(&options)
        .await
        .expect("failed to open an mbtiles file");
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT name, value FROM metadata")
        .fetch_all(&mut conn)
        .await
        .expect("failed to read the metadata table");
    conn.close().await.expect("failed to close an mbtiles file");
    rows.into_iter().collect()
}

fn summary(source: &Path) -> MbtilesCli {
    MbtilesCli::new("summary")
        .arg("--format")
        .arg("json")
        .arg(source)
}

#[tokio::test]
async fn summary_reports_the_schema_and_per_zoom_statistics() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;

    let summary = summary(&source).run_json().await;

    assert_eq!(summary["mbt_type"], "Flat");
    assert_eq!(summary["tile_count"], 8);
    assert_eq!(summary["min_zoom"], 0);
    assert_eq!(summary["max_zoom"], 6);
    assert_eq!(summary["min_tile_size"], 20);
    assert_eq!(summary["max_tile_size"], 1107);

    let per_zoom = summary["zoom_info"]
        .as_array()
        .expect("the summary reports one entry per zoom level");
    assert_eq!(per_zoom.len(), 7);
    assert_eq!(
        per_zoom
            .iter()
            .map(|zoom| zoom["tile_count"]
                .as_u64()
                .expect("a tile count is a number"))
            .sum::<u64>(),
        summary["tile_count"]
            .as_u64()
            .expect("a tile count is a number"),
        "the per-zoom counts must add up to the total"
    );

    // Zoom 2 is the only level holding more than one tile, so it is the only one
    // where the smallest, largest and average sizes can differ.
    let zoom2 = per_zoom
        .iter()
        .find(|zoom| zoom["zoom"] == 2)
        .expect("zoom 2 holds tiles");
    assert_eq!(zoom2["tile_count"], 2);
    assert_eq!(zoom2["min_tile_size"], 151);
    assert_eq!(zoom2["max_tile_size"], 263);
    assert_eq!(zoom2["avg_tile_size"], 207.0);
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

    assert!(output.contains("Schema:         flat"), "{output}");
    for zoom in 0..=6 {
        assert!(
            output.contains(&format!("\n    {zoom} |")),
            "the table has a row for zoom {zoom}:\n{output}"
        );
    }
    assert!(
        output.contains("\n  all |         8 |"),
        "the table ends with a total row:\n{output}"
    );
}

#[tokio::test]
async fn meta_all_prints_every_metadata_row() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;

    let output = MbtilesCli::new("meta-all").arg(&source).run().await;

    let metadata = metadata(&source).await;
    assert!(metadata.contains_key("agg_tiles_hash"));
    for key in metadata.keys() {
        assert!(output.contains(key), "meta-all left out `{key}`:\n{output}");
    }
    // `json` is unpacked into the tilejson block rather than printed verbatim.
    assert!(output.contains("vector_layers:"), "{output}");
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

    assert_eq!(output.trim_end(), "Major cities from Natural Earth data");
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

    assert!(output.is_empty(), "unexpected output:\n{output}");
}

#[tokio::test]
async fn meta_all_documents_itself() {
    let help = MbtilesCli::new("meta-all").arg("--help").run().await;

    insta::assert_snapshot!(help, @r"
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

    insta::assert_snapshot!(help, @r"
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
#[case::flat_with_hash("zoomed_world_cities", "AC15E26A1FCF82FDB6D0E8F43EE37821")]
#[case::normalized_with_duplicate_ids("normalized-dedup-id", "3CE4DB27DDC5A385756CC384CDAFC3D5")]
#[tokio::test]
async fn validate_accepts_a_file_whose_hashes_match(#[case] name: &str, #[case] agg_hash: &str) {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), name).await;

    let output = MbtilesCli::new("validate").arg(&source).run().await;

    assert!(output.contains("Integrity check passed"), "{output}");
    assert!(
        output.contains("All values in the `tiles` table/view are valid"),
        "{output}"
    );
    assert!(output.contains("All tile hashes are valid"), "{output}");
    assert!(
        output.contains("agg_tiles_hash has been verified"),
        "{output}"
    );
    assert!(output.contains(agg_hash), "{output}");
}

#[tokio::test]
async fn validate_rejects_a_tile_outside_its_zoom_level() {
    let dir = temp_dir();
    let source = broken_fixture(dir.path(), "invalid-tile-idx").await;

    let output = MbtilesCli::new("validate").arg(&source).run_failing().await;

    assert!(
        output.contains(
            "At least one tile in the tiles table/view has an invalid value: \
             zoom_level=6, tile_column=10, tile_row=64"
        ),
        "{output}"
    );
}

#[tokio::test]
async fn validate_rejects_a_stale_aggregate_hash() {
    let dir = temp_dir();
    let source = broken_fixture(dir.path(), "bad_hash").await;

    let output = MbtilesCli::new("validate").arg(&source).run_failing().await;

    // The per-tile hashes are fine here; only the file-wide roll-up is wrong.
    assert!(output.contains("All tile hashes are valid"), "{output}");
    assert!(
        output.contains(&format!(
            "Computed aggregate tiles hash {REAL_AGG_HASH} does not match tile data in metadata {STALE_AGG_HASH}"
        )),
        "{output}"
    );
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

    assert!(
        output.contains(&format!(
            "Updating agg_tiles_hash mbtiles.file={} agg_tiles_hash.old={STALE_AGG_HASH} agg_tiles_hash.new={REAL_AGG_HASH}",
            source.display()
        )),
        "{output}"
    );
    assert_eq!(metadata(&source).await["agg_tiles_hash"], REAL_AGG_HASH);

    let output = MbtilesCli::new("validate").arg(&source).run().await;
    assert!(
        output.contains("agg_tiles_hash has been verified"),
        "{output}"
    );
}

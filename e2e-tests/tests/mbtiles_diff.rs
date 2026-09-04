//! `copy`, `diff`, `apply-patch` and `cache-purge` in the `mbtiles` CLI: moving a tileset
//! between schemas, and the patch files that turn one tileset into another.

use std::fs;
use std::path::{Path, PathBuf};

use martin_e2e_tests::{
    MbtilesCli, Tile, gunzip, mbtiles_fixture, metadata, open_read_only, open_read_write,
    patch_tiles, summary, summary_filters, temp_dir, tiles,
};
use regex::Regex;
use sqlx::Connection as _;
use tempfile::TempDir;

/// The tiles of `path` with any gzip wrapper taken off.
///
/// `apply-patch` re-gzips the tiles it rebuilds, and says so in its own log: the compressed
/// bytes are not guaranteed to match the tileset the patch was cut from, only their contents.
async fn plain_tiles(path: &Path) -> Vec<Tile> {
    tiles(path)
        .await
        .into_iter()
        .map(|(z, x, y, data)| (z, x, y, gunzip(&data)))
        .collect()
}

/// The bsdiff payloads of a bin-diff patch file.
///
/// Modified tiles travel here rather than in `tiles`, so a patch comparison that
/// stops at `tiles` and `metadata` would miss them.
async fn bsdiff_rows(path: &Path) -> Vec<(i64, i64, i64, Vec<u8>, i64)> {
    let mut conn = open_read_only(path).await;
    let rows = sqlx::query_as(
        "SELECT zoom_level, tile_column, tile_row, patch_data, tile_xxh3_64_hash \
         FROM bsdiffrawgz ORDER BY 1, 2, 3",
    )
    .fetch_all(&mut conn)
    .await
    .expect("failed to read the bsdiffrawgz table");
    conn.close().await.expect("failed to close an mbtiles file");
    rows
}

/// A run's output with the parts that are not the same on every machine replaced, so that it can
/// be snapshotted: this run's temp directory, and the number of cpus the bindiff pass found.
fn redact(dir: &TempDir, output: &str) -> String {
    let output = output.replace(&dir.path().display().to_string(), "[TMP]");
    let output = output.replace(std::path::MAIN_SEPARATOR, "/");
    Regex::new(r"bindiff\.cpus=\d+")
        .expect("the pattern is valid")
        .replace_all(&output, "bindiff.cpus=[CPUS]")
        .into_owned()
}

/// Build `world_cities` and `world_cities_modified` in `dir` and cut a patch between them.
/// Returns the tileset the patch applies to, the one it produces, and the patch itself.
async fn patch_fixtures(dir: &Path, extra_args: &[&str]) -> (PathBuf, PathBuf, PathBuf) {
    let source = mbtiles_fixture(dir, "world_cities").await;
    let modified = mbtiles_fixture(dir, "world_cities_modified").await;
    let patch = dir.join("patch.mbtiles");

    let mut command = MbtilesCli::new("copy")
        .arg(&source)
        .arg(&patch)
        .arg("--diff-with-file")
        .arg(&modified);
    for arg in extra_args {
        command = command.arg(*arg);
    }
    command.run().await;

    (source, modified, patch)
}

#[tokio::test]
async fn copying_through_the_cache_schema_keeps_every_tile() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let cache = dir.path().join("cache.mbtiles");
    let back = dir.path().join("back.mbtiles");

    let output = MbtilesCli::new("copy")
        .arg(&source)
        .arg(&cache)
        .arg("--mbtiles-type")
        .arg("cache")
        .run()
        .await;
    insta::assert_snapshot!(redact(&dir, &output), @"
    INFO copy: Copying [TMP]/world_cities.mbtiles (flat) to a new file [TMP]/cache.mbtiles (cache)
    INFO copy: Updating agg_tiles_hash mbtiles.file=[TMP]/cache.mbtiles agg_tiles_hash.old=84792BF4EE9AEDDC5B1A60E707011FEE agg_tiles_hash.new=08934F8E3E58DED510920E1DE6F4E78F
    ");

    let summary = summary(&cache).run_json().await;
    insta::with_settings!({filters => summary_filters()}, {
        insta::assert_json_snapshot!("the_summary_of_the_cache_copy", summary);
    });

    MbtilesCli::new("copy")
        .arg(&cache)
        .arg(&back)
        .arg("--mbtiles-type")
        .arg("flat")
        .run()
        .await;

    assert_eq!(tiles(&source).await, tiles(&back).await);
}

#[tokio::test]
async fn a_cache_round_trip_leaves_the_differ_nothing_to_report() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let cache = dir.path().join("cache.mbtiles");
    let back = dir.path().join("back.mbtiles");
    let patch = dir.path().join("patch.mbtiles");

    MbtilesCli::new("copy")
        .arg(&source)
        .arg(&cache)
        .arg("--mbtiles-type")
        .arg("cache")
        .run()
        .await;
    MbtilesCli::new("copy")
        .arg(&cache)
        .arg(&back)
        .arg("--mbtiles-type")
        .arg("flat")
        .run()
        .await;
    MbtilesCli::new("copy")
        .arg(&source)
        .arg(&patch)
        .arg("--diff-with-file")
        .arg(&back)
        .run()
        .await;

    assert!(patch_tiles(&patch).await.is_empty());
    // The hash of an empty tileset, which is how the differ reports "no changes".
    assert_eq!(
        metadata(&patch).await["agg_tiles_hash"],
        "D41D8CD98F00B204E9800998ECF8427E"
    );
}

#[tokio::test]
async fn validating_a_cache_file_skips_the_per_tile_hashes() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let cache = dir.path().join("cache.mbtiles");

    MbtilesCli::new("copy")
        .arg(&source)
        .arg(&cache)
        .arg("--mbtiles-type")
        .arg("cache")
        .run()
        .await;
    let output = MbtilesCli::new("validate").arg(&cache).run().await;

    insta::assert_snapshot!(redact(&dir, &output), @"
    INFO validate: Integrity check passed mbtiles.file=[TMP]/cache.mbtiles integrity_check=Quick
    INFO validate: All values in the `tiles` table/view are valid mbtiles.file=[TMP]/cache.mbtiles
    INFO validate: Skipping per-tile hash validation because this is a cache MBTiles file mbtiles.file=[TMP]/cache.mbtiles
    INFO validate: agg_tiles_hash has been verified mbtiles.file=[TMP]/cache.mbtiles agg_tiles_hash=08934F8E3E58DED510920E1DE6F4E78F
    ");
}

#[tokio::test]
async fn purging_a_cache_keeps_the_entries_that_have_not_expired() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let cache = dir.path().join("cache.mbtiles");

    MbtilesCli::new("copy")
        .arg(&source)
        .arg(&cache)
        .arg("--mbtiles-type")
        .arg("cache")
        .run()
        .await;
    let output = MbtilesCli::new("cache-purge").arg(&cache).run().await;

    insta::assert_snapshot!(redact(&dir, &output), @"Removed 0 expired tile entries");
    assert_eq!(tiles(&cache).await.len(), 8);
}

#[tokio::test]
async fn purging_refuses_a_tileset_that_is_not_a_cache() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let before = tiles(&source).await;

    let output = MbtilesCli::new("cache-purge")
        .arg(&source)
        .run_failing()
        .await;

    insta::assert_snapshot!(redact(&dir, &output), @"ERROR File [TMP]/world_cities.mbtiles exists but does not use the tile-cache schema, refusing to modify it. Use an empty/new file or an existing cache file");
    assert_eq!(before, tiles(&source).await, "the file must be left alone");
}

#[tokio::test]
async fn diff_and_copy_with_diff_with_file_write_the_same_patch() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let modified = mbtiles_fixture(dir.path(), "world_cities_modified").await;
    let from_copy = dir.path().join("from_copy.mbtiles");
    let from_diff = dir.path().join("from_diff.mbtiles");

    MbtilesCli::new("copy")
        .arg(&source)
        .arg(&from_copy)
        .arg("--diff-with-file")
        .arg(&modified)
        .run()
        .await;
    MbtilesCli::new("diff")
        .arg(&source)
        .arg(&modified)
        .arg(&from_diff)
        .run()
        .await;

    assert!(
        !patch_tiles(&from_copy).await.is_empty(),
        "the patch is empty"
    );
    assert_eq!(patch_tiles(&from_copy).await, patch_tiles(&from_diff).await);
    assert_eq!(metadata(&from_copy).await, metadata(&from_diff).await);
}

#[tokio::test]
async fn a_patch_applied_as_plain_sql_reproduces_the_modified_tileset() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let modified = mbtiles_fixture(dir.path(), "world_cities_modified").await;
    let patch = dir.path().join("patch.mbtiles");
    let applied = dir.path().join("applied.mbtiles");
    fs::copy(&source, &applied).expect("failed to copy the tileset to patch");
    assert_ne!(
        tiles(&source).await,
        tiles(&modified).await,
        "the two fixtures must differ, or the patch proves nothing"
    );

    MbtilesCli::new("copy")
        .arg(&source)
        .arg(&patch)
        .arg("--diff-with-file")
        .arg(&modified)
        .run()
        .await;

    // A plain diff is meant to be applicable without the CLI: deletions are NULL tiles,
    // everything else is an upsert.
    let mut conn = open_read_write(&applied).await;
    sqlx::query("ATTACH DATABASE ? AS diffDb")
        .bind(patch.to_str().expect("a temp path is utf-8"))
        .execute(&mut conn)
        .await
        .expect("failed to attach the patch");
    sqlx::query(
        "DELETE FROM tiles WHERE (zoom_level, tile_column, tile_row) IN \
         (SELECT zoom_level, tile_column, tile_row FROM diffDb.tiles WHERE tile_data ISNULL)",
    )
    .execute(&mut conn)
    .await
    .expect("failed to apply the deletions");
    sqlx::query(
        "INSERT OR REPLACE INTO tiles (zoom_level, tile_column, tile_row, tile_data) \
         SELECT * FROM diffDb.tiles WHERE tile_data NOTNULL",
    )
    .execute(&mut conn)
    .await
    .expect("failed to apply the additions");
    conn.close().await.expect("failed to close an mbtiles file");

    assert_eq!(tiles(&modified).await, tiles(&applied).await);
}

#[tokio::test]
async fn a_bin_diff_patch_reproduces_the_modified_tileset() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let modified = mbtiles_fixture(dir.path(), "world_cities_modified").await;
    let patch = dir.path().join("patch.mbtiles");
    let applied = dir.path().join("applied.mbtiles");
    assert_ne!(
        tiles(&source).await,
        tiles(&modified).await,
        "the two fixtures must differ, or the patch proves nothing"
    );

    MbtilesCli::new("copy")
        .arg(&source)
        .arg(&patch)
        .arg("--diff-with-file")
        .arg(&modified)
        .arg("--patch-type")
        .arg("bin-diff-gz")
        .run()
        .await;
    MbtilesCli::new("copy")
        .arg(&source)
        .arg(&applied)
        .arg("--apply-patch")
        .arg(&patch)
        .run()
        .await;

    assert_eq!(plain_tiles(&modified).await, plain_tiles(&applied).await);
}

#[tokio::test]
async fn the_checked_in_bin_diff_fixture_still_applies() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let modified = mbtiles_fixture(dir.path(), "world_cities_modified").await;
    let patch = mbtiles_fixture(dir.path(), "world_cities_bindiff").await;
    let applied = dir.path().join("applied.mbtiles");
    assert_ne!(
        tiles(&source).await,
        tiles(&modified).await,
        "the two fixtures must differ, or the patch proves nothing"
    );

    MbtilesCli::new("copy")
        .arg(&source)
        .arg(&applied)
        .arg("--apply-patch")
        .arg(&patch)
        .run()
        .await;

    assert_eq!(plain_tiles(&modified).await, plain_tiles(&applied).await);
}

#[tokio::test]
async fn a_freshly_cut_bin_diff_matches_the_checked_in_one() {
    let dir = temp_dir();
    let (_, _, cut) = patch_fixtures(dir.path(), &["--patch-type", "bin-diff-gz"]).await;
    let checked_in = mbtiles_fixture(dir.path(), "world_cities_bindiff").await;

    assert_eq!(patch_tiles(&cut).await, patch_tiles(&checked_in).await);
    assert_eq!(bsdiff_rows(&cut).await, bsdiff_rows(&checked_in).await);
    assert_eq!(metadata(&cut).await, metadata(&checked_in).await);
}

#[tokio::test]
async fn applying_a_patch_announces_the_hashes_it_expects() {
    let dir = temp_dir();
    let (source, _, patch) = patch_fixtures(dir.path(), &["--patch-type", "bin-diff-gz"]).await;
    let applied = dir.path().join("applied.mbtiles");

    let output = MbtilesCli::new("copy")
        .arg(&source)
        .arg(&applied)
        .arg("--apply-patch")
        .arg(&patch)
        .run()
        .await;

    // Re-gzip-ing may change the bytes, so the resulting hash is deliberately not validated,
    // and the run says so.
    insta::assert_snapshot!(redact(&dir, &output), @"
    INFO copy: The patch file [TMP]/patch.mbtiles expects to be applied to a tileset with agg_tiles_hash=84792BF4EE9AEDDC5B1A60E707011FEE, and should result in hash 578FB5BD64746C39E3D344662947FD0D after applying
    INFO copy: Applying patch from [TMP]/patch.mbtiles (flat) to [TMP]/world_cities.mbtiles (flat) into a new file [TMP]/applied.mbtiles (flat) with bin-diff on gzip-ed tiles
    INFO copy: Processing bindiff patches bindiff.cpus=[CPUS]
    INFO copy: Finished processing bindiff tiles bindiff.inserted=0
    INFO copy: Adding a new metadata value agg_tiles_hash mbtiles.file=[TMP]/applied.mbtiles agg_tiles_hash=72D8C992AF67EC97093B0087933FA160
    INFO copy: Skipping agg_tiles_hash_after_apply validation because re-gzip-ing could produce different tile data. Each bindiff-ed tile was still verified with a hash value
    ");
}

#[tokio::test]
async fn bin_diff_reports_how_many_workers_it_used() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let modified = mbtiles_fixture(dir.path(), "world_cities_modified").await;
    let patch = dir.path().join("patch.mbtiles");
    let applied = dir.path().join("applied.mbtiles");

    // Both cutting and applying a bin-diff run the parallel bindiff pass and say so.
    let cutting = MbtilesCli::new("copy")
        .arg(&source)
        .arg(&patch)
        .arg("--diff-with-file")
        .arg(&modified)
        .arg("--patch-type")
        .arg("bin-diff-gz")
        .run()
        .await;
    insta::assert_snapshot!("cutting_a_bin_diff", redact(&dir, &cutting));

    let applying = MbtilesCli::new("copy")
        .arg(&source)
        .arg(&applied)
        .arg("--apply-patch")
        .arg(&patch)
        .run()
        .await;
    insta::assert_snapshot!("applying_a_bin_diff", redact(&dir, &applying));
}

//! `copy`, `diff`, `apply-patch` and `cache-purge` in the `mbtiles` CLI: moving a tileset
//! between schemas, and the patch files that turn one tileset into another.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use martin_integration_tests::{MbtilesCli, mbtiles_fixture};
use regex::Regex;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection as _, SqliteConnection};
use tempfile::TempDir;

const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// A row of a `tiles` table. A patch file stores deletions as a `NULL` tile.
type Tile = (i64, i64, i64, Option<Vec<u8>>);

fn temp_dir() -> TempDir {
    tempfile::tempdir().expect("failed to create a temp dir")
}

async fn open(path: &Path, read_only: bool) -> SqliteConnection {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(read_only);
    SqliteConnection::connect_with(&options)
        .await
        .expect("failed to open an mbtiles file")
}

async fn tiles(path: &Path) -> Vec<Tile> {
    let mut conn = open(path, true).await;
    let rows = sqlx::query_as(
        "SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles ORDER BY 1, 2, 3",
    )
    .fetch_all(&mut conn)
    .await
    .expect("failed to read the tiles table");
    conn.close().await.expect("failed to close an mbtiles file");
    rows
}

/// The tiles of `path` with any gzip wrapper taken off.
///
/// `apply-patch` re-gzips the tiles it rebuilds, and says so in its own log: the compressed
/// bytes are not guaranteed to match the tileset the patch was cut from, only their contents.
async fn plain_tiles(path: &Path) -> Vec<Tile> {
    tiles(path)
        .await
        .into_iter()
        .map(|(z, x, y, data)| (z, x, y, data.as_deref().map(gunzip)))
        .collect()
}

fn gunzip(data: &[u8]) -> Vec<u8> {
    if !data.starts_with(&GZIP_MAGIC) {
        return data.to_vec();
    }
    let mut plain = Vec::new();
    GzDecoder::new(data)
        .read_to_end(&mut plain)
        .expect("failed to decompress a tile");
    plain
}

/// The bsdiff payloads of a bin-diff patch file.
///
/// Modified tiles travel here rather than in `tiles`, so a patch comparison that
/// stops at `tiles` and `metadata` would miss them.
async fn bsdiff_rows(path: &Path) -> Vec<(i64, i64, i64, Vec<u8>, i64)> {
    let mut conn = open(path, true).await;
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

async fn metadata(path: &Path) -> BTreeMap<String, String> {
    let mut conn = open(path, true).await;
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT name, value FROM metadata")
        .fetch_all(&mut conn)
        .await
        .expect("failed to read the metadata table");
    conn.close().await.expect("failed to close an mbtiles file");
    rows.into_iter().collect()
}

/// Build `world_cities` and `world_cities_modified` in `dir` and cut a patch between them.
/// Returns the tileset the patch applies to, the one it produces, and the patch itself.
async fn patch_fixtures(dir: &Path, extra_args: &[&str]) -> (PathBuf, PathBuf, PathBuf) {
    let source = mbtiles_fixture(dir, "world_cities").await;
    let modified = mbtiles_fixture(dir, "world_cities_modified").await;
    let patch = dir.join("patch.mbtiles");

    let mut command = MbtilesCli::new("copy")
        .arg(&source)
        .arg("--diff-with-file")
        .arg(&modified)
        .arg(&patch);
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
    assert!(output.contains("(flat) to a new file"), "{output}");
    assert!(output.contains("(cache)"), "{output}");

    let summary = MbtilesCli::new("summary")
        .arg("--format")
        .arg("json")
        .arg(&cache)
        .run_json()
        .await;
    assert_eq!(summary["mbt_type"], "Cache");
    assert_eq!(summary["tile_count"], 8);

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
        .arg("--diff-with-file")
        .arg(&back)
        .arg(&patch)
        .run()
        .await;

    assert!(tiles(&patch).await.is_empty());
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

    assert!(
        output.contains("Skipping per-tile hash validation because this is a cache MBTiles file"),
        "{output}"
    );
    assert!(
        output.contains("agg_tiles_hash has been verified"),
        "{output}"
    );
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

    assert!(
        output.contains("Removed 0 expired tile entries"),
        "{output}"
    );
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

    assert!(
        output.contains("does not use the tile-cache schema, refusing to modify it"),
        "{output}"
    );
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
        .arg("--diff-with-file")
        .arg(&modified)
        .arg(&from_copy)
        .run()
        .await;
    MbtilesCli::new("diff")
        .arg(&source)
        .arg(&modified)
        .arg(&from_diff)
        .run()
        .await;

    assert!(!tiles(&from_copy).await.is_empty(), "the patch is empty");
    assert_eq!(tiles(&from_copy).await, tiles(&from_diff).await);
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
        .arg("--diff-with-file")
        .arg(&modified)
        .arg(&patch)
        .run()
        .await;

    // A plain diff is meant to be applicable without the CLI: deletions are NULL tiles,
    // everything else is an upsert.
    let mut conn = open(&applied, false).await;
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
        .arg("--diff-with-file")
        .arg(&modified)
        .arg(&patch)
        .arg("--patch-type")
        .arg("bin-diff-gz")
        .run()
        .await;
    MbtilesCli::new("copy")
        .arg(&source)
        .arg("--apply-patch")
        .arg(&patch)
        .arg(&applied)
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
        .arg("--apply-patch")
        .arg(&patch)
        .arg(&applied)
        .run()
        .await;

    assert_eq!(plain_tiles(&modified).await, plain_tiles(&applied).await);
}

#[tokio::test]
async fn a_freshly_cut_bin_diff_matches_the_checked_in_one() {
    let dir = temp_dir();
    let (_, _, cut) = patch_fixtures(dir.path(), &["--patch-type", "bin-diff-gz"]).await;
    let checked_in = mbtiles_fixture(dir.path(), "world_cities_bindiff").await;

    assert_eq!(tiles(&cut).await, tiles(&checked_in).await);
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
        .arg("--apply-patch")
        .arg(&patch)
        .arg(&applied)
        .run()
        .await;

    assert!(
        output.contains(
            "expects to be applied to a tileset with agg_tiles_hash=84792BF4EE9AEDDC5B1A60E707011FEE, \
             and should result in hash 578FB5BD64746C39E3D344662947FD0D after applying"
        ),
        "{output}"
    );
    // Re-gzip-ing may change the bytes, so the resulting hash is deliberately not checked.
    assert!(
        output.contains("Skipping agg_tiles_hash_after_apply validation"),
        "{output}"
    );
}

#[tokio::test]
async fn bin_diff_reports_how_many_workers_it_used() {
    let dir = temp_dir();
    let source = mbtiles_fixture(dir.path(), "world_cities").await;
    let modified = mbtiles_fixture(dir.path(), "world_cities_modified").await;
    let patch = dir.path().join("patch.mbtiles");
    let applied = dir.path().join("applied.mbtiles");
    let workers = Regex::new(r"Processing bindiff patches bindiff\.cpus=[1-9][0-9]*")
        .expect("the pattern is valid");

    // Both cutting and applying a bin-diff run the parallel bindiff pass and say so.
    let cutting = MbtilesCli::new("copy")
        .arg(&source)
        .arg("--diff-with-file")
        .arg(&modified)
        .arg(&patch)
        .arg("--patch-type")
        .arg("bin-diff-gz")
        .run()
        .await;
    assert!(workers.is_match(&cutting), "{cutting}");

    let applying = MbtilesCli::new("copy")
        .arg(&source)
        .arg("--apply-patch")
        .arg(&patch)
        .arg(&applied)
        .run()
        .await;
    assert!(workers.is_match(&applying), "{applying}");
    assert!(
        applying.contains("Finished processing bindiff tiles bindiff.inserted=0"),
        "{applying}"
    );
}

//! `agg_tiles_hash` checks done by `diff`, `copy --apply-patch` and `apply-patch` (#1244)
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use mbtiles::{HashAlgorithm, MbtError, MbtTypeCli, Mbtiles, MbtilesCopier, apply_patch, temp_named_mbtiles};
use sqlx::{AssertSqlSafe, Executor as _, SqliteConnection, query_scalar};

const WORLD_CITIES: &str = include_str!("../../tests/fixtures/mbtiles/world_cities.sql");
const WORLD_CITIES_MODIFIED: &str = include_str!("../../tests/fixtures/mbtiles/world_cities_modified.sql");
const WORLD_CITIES_DIFF: &str = include_str!("../../tests/fixtures/mbtiles/world_cities_diff.sql");

const HASH_A: &str = "08934F8E3E58DED510920E1DE6F4E78F";
const HASH_B: &str = "72D8C992AF67EC97093B0087933FA160";

/// In-memory file loaded from `script`; keep the connection alive or the db goes away
async fn load(name: &str, script: &str) -> (SqliteConnection, PathBuf) {
    let (_mbt, conn, path) = temp_named_mbtiles(name, script).await;
    (conn, path)
}

async fn exec(conn: &mut SqliteConnection, sql: &str) {
    conn.execute(AssertSqlSafe(sql)).await.unwrap();
}

async fn meta(conn: &mut SqliteConnection, key: &str) -> Option<String> {
    query_scalar("SELECT value FROM metadata WHERE name = ?")
        .bind(key)
        .fetch_optional(conn)
        .await
        .unwrap()
}

fn mem(name: &str) -> PathBuf {
    PathBuf::from(format!("file:{name}?mode=memory&cache=shared"))
}

async fn diff(src: PathBuf, other: PathBuf, dst: &str) -> Result<SqliteConnection, MbtError> {
    MbtilesCopier {
        src_file: src,
        dst_file: mem(dst),
        diff_with_file: Some((other, None)),
        ..Default::default()
    }
    .run()
    .await
}

#[actix_rt::test]
async fn diff_computes_missing_hashes() {
    let (mut a, a_path) = load("dv_missing_a", WORLD_CITIES).await;
    let (mut b, b_path) = load("dv_missing_b", WORLD_CITIES_MODIFIED).await;
    exec(&mut a, "DELETE FROM metadata WHERE name = 'agg_tiles_hash'").await;
    exec(&mut b, "DELETE FROM metadata WHERE name = 'agg_tiles_hash'").await;

    let mut d = MbtilesCopier {
        src_file: a_path,
        dst_file: mem("dv_missing_d"),
        diff_with_file: Some((b_path, None)),
        validate: true,
        ..Default::default()
    }
    .run()
    .await
    .unwrap();

    assert_eq!(
        meta(&mut d, "agg_tiles_hash_before_apply").await.as_deref(),
        Some(HASH_A)
    );
    assert_eq!(
        meta(&mut d, "agg_tiles_hash_after_apply").await.as_deref(),
        Some(HASH_B)
    );
    assert_eq!(meta(&mut a, "agg_tiles_hash").await, None, "input must not be modified");
}

#[actix_rt::test]
async fn diff_refuses_patch_file_as_input() {
    let (_a, a_path) = load("dv_patch_input_a", WORLD_CITIES_DIFF).await;
    let (_b, b_path) = load("dv_patch_input_b", WORLD_CITIES_MODIFIED).await;

    let err = diff(a_path, b_path, "dv_patch_input_d").await.unwrap_err();
    assert!(matches!(err, MbtError::DiffingDiffFile(_)), "{err:?}");
}

async fn patch_on_copy(src: PathBuf, patch: PathBuf, dst: &str, force: bool) -> Result<SqliteConnection, MbtError> {
    MbtilesCopier {
        src_file: src,
        dst_file: mem(dst),
        apply_patch: Some(patch),
        force,
        ..Default::default()
    }
    .run()
    .await
}

#[actix_rt::test]
async fn copy_apply_patch_refuses_wrong_base_before_writing() {
    let (_b, b_path) = load("dv_wrong_base_b", WORLD_CITIES_MODIFIED).await;
    let (_d, d_path) = load("dv_wrong_base_d", WORLD_CITIES_DIFF).await;

    let err = patch_on_copy(b_path, d_path, "dv_wrong_base_out", false)
        .await
        .unwrap_err();
    assert!(matches!(err, MbtError::AggHashMismatchWithDiff { .. }), "{err:?}");
}

#[actix_rt::test]
async fn copy_apply_patch_refuses_diff_without_own_hash() {
    let (_a, a_path) = load("dv_no_own_hash_a", WORLD_CITIES).await;
    let (mut d, d_path) = load("dv_no_own_hash_d", WORLD_CITIES_DIFF).await;
    exec(&mut d, "DELETE FROM metadata WHERE name = 'agg_tiles_hash'").await;

    let err = patch_on_copy(a_path.clone(), d_path.clone(), "dv_no_own_hash_out", false)
        .await
        .unwrap_err();
    assert!(matches!(err, MbtError::PatchFileHasNoAggHash(_)), "{err:?}");

    patch_on_copy(a_path, d_path, "dv_no_own_hash_out_forced", true)
        .await
        .unwrap();
}

async fn computed_hash(path: &PathBuf) -> String {
    let mbt = Mbtiles::new(path).unwrap();
    let mut conn = mbt.open_readonly().await.unwrap();
    mbt.resolve_agg_tiles_hash(&mut conn, true).await.unwrap()
}

#[actix_rt::test]
async fn apply_patch_checks_base_without_stored_hash() {
    let (mut b, b_path) = load("dv_inplace_nohash_b", WORLD_CITIES_MODIFIED).await;
    let (_d, d_path) = load("dv_inplace_nohash_d", WORLD_CITIES_DIFF).await;
    exec(&mut b, "DELETE FROM metadata WHERE name = 'agg_tiles_hash'").await;

    let err = apply_patch(b_path, d_path, false, false).await.unwrap_err();
    assert!(matches!(err, MbtError::AggHashMismatchWithDiff { .. }), "{err:?}");
}

#[actix_rt::test]
async fn apply_patch_checks_after_apply_hash() {
    let (mut a, a_path) = load("dv_after_a", WORLD_CITIES).await;
    let (mut d, d_path) = load("dv_after_d", WORLD_CITIES_DIFF).await;
    exec(
        &mut d,
        "UPDATE metadata SET value = 'DEADBEEF' WHERE name = 'agg_tiles_hash_after_apply'",
    )
    .await;

    let err = apply_patch(a_path.clone(), d_path.clone(), false, false)
        .await
        .unwrap_err();
    assert!(matches!(err, MbtError::AggHashMismatchAfterApply { .. }), "{err:?}");
    assert_eq!(computed_hash(&a_path).await, HASH_A, "tiles must be rolled back");
    assert_eq!(meta(&mut a, "agg_tiles_hash").await.as_deref(), Some(HASH_A));

    // with --force it goes through, but stores the real hash rather than the patch's claim
    apply_patch(a_path, d_path, true, false).await.unwrap();
    assert_eq!(meta(&mut a, "agg_tiles_hash").await.as_deref(), Some(HASH_B));
}

#[actix_rt::test]
async fn apply_patch_stale_base_hash() {
    let (mut a, a_path) = load("dv_stale_a", WORLD_CITIES).await;
    let (_d, d_path) = load("dv_stale_d", WORLD_CITIES_DIFF).await;
    // z1 isn't touched by the diff, so the patched result can't accidentally match
    exec(
        &mut a,
        "DELETE FROM tiles WHERE zoom_level = 1 AND tile_column = 0 AND tile_row = 0",
    )
    .await;
    let edited = computed_hash(&a_path).await;

    let err = apply_patch(a_path.clone(), d_path.clone(), false, true)
        .await
        .unwrap_err();
    assert!(matches!(err, MbtError::AggHashMismatchWithDiff { .. }), "{err:?}");

    let err = apply_patch(a_path.clone(), d_path, false, false).await.unwrap_err();
    assert!(matches!(err, MbtError::AggHashMismatchAfterApply { .. }), "{err:?}");
    assert_eq!(computed_hash(&a_path).await, edited, "tiles must be rolled back");
}

#[actix_rt::test]
async fn diff_and_patch_across_hash_algorithms() {
    let (_a, a_flat) = load("dv_algo_a_flat", WORLD_CITIES).await;
    let (_b, b_flat) = load("dv_algo_b_flat", WORLD_CITIES_MODIFIED).await;
    let copy = |src: PathBuf, dst: &str, hash_algorithm| MbtilesCopier {
        src_file: src,
        dst_file: mem(dst),
        dst_type_cli: Some(MbtTypeCli::FlatWithHash),
        hash_algorithm,
        ..Default::default()
    };
    let _a = copy(a_flat, "dv_algo_a", None).run().await.unwrap();
    let _b = copy(b_flat, "dv_algo_b", Some(HashAlgorithm::Xxh3))
        .run()
        .await
        .unwrap();

    // validate re-checks every tile_hash in the diff against its declared algorithm
    let _d = MbtilesCopier {
        src_file: mem("dv_algo_a"),
        dst_file: mem("dv_algo_d"),
        diff_with_file: Some((mem("dv_algo_b"), None)),
        validate: true,
        ..Default::default()
    }
    .run()
    .await
    .unwrap();

    apply_patch(mem("dv_algo_a"), mem("dv_algo_d"), false, false)
        .await
        .unwrap();
    assert_eq!(computed_hash(&mem("dv_algo_a")).await, HASH_B);
}

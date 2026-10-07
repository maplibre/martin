#![allow(clippy::unwrap_used)]
use std::assert_matches;
use std::fmt::Write as _;

use mbtiles::AggHashType::Update;
use mbtiles::IntegrityCheckType::Full;
use mbtiles::{
    BulkTile, CopyDuplicateMode, DedupHint, MbtError, MbtType, Mbtiles, MbtilesBulkWriter,
    NormalizedSchema, TileCoord, action_with_rusqlite, init_mbtiles_schema,
};
use rstest::rstest;
use sqlx::{AssertSqlSafe, Row as _, SqliteConnection, query};

const DEDUP_ID: MbtType = MbtType::Normalized {
    hash_view: false,
    schema: NormalizedSchema::DedupId,
};

fn sample_tiles() -> Vec<BulkTile<Vec<u8>>> {
    let mut tiles = Vec::new();
    for z in 0..=4_u8 {
        for x in 0..(1_u32 << z) {
            for y in 0..(1_u32 << z) {
                let (data, hint) = if (x + y) % 3 == 0 {
                    (b"ocean".to_vec(), DedupHint::LikelyDuplicate)
                } else if (x + 2 * y) % 5 == 0 {
                    (Vec::new(), DedupHint::LikelyDuplicate)
                } else {
                    (format!("tile {z}/{x}/{y}").into_bytes(), DedupHint::Unique)
                };
                tiles.push(BulkTile {
                    coord: TileCoord::new_unchecked(z, x, y),
                    data,
                    hint,
                });
            }
        }
    }
    tiles
}

async fn new_mbtiles(mbt_type: MbtType) -> (Mbtiles, SqliteConnection) {
    let mbt = Mbtiles::new(":memory:").unwrap();
    let mut conn = mbt.open().await.unwrap();
    init_mbtiles_schema(&mut conn, mbt_type, false)
        .await
        .unwrap();
    (mbt, conn)
}

async fn via_insert_tiles(mbt_type: MbtType, tiles: &[BulkTile<Vec<u8>>]) -> SqliteConnection {
    let (mbt, mut conn) = new_mbtiles(mbt_type).await;
    for chunk in tiles.chunks(100) {
        let batch: Vec<(u8, u32, u32, Vec<u8>)> = chunk.iter().map(Into::into).collect();
        mbt.insert_tiles(&mut conn, mbt_type, CopyDuplicateMode::Abort, &batch)
            .await
            .unwrap();
    }
    conn
}

async fn via_bulk_writer(mbt_type: MbtType, tiles: &[BulkTile<Vec<u8>>]) -> SqliteConnection {
    let (_, mut conn) = new_mbtiles(mbt_type).await;
    action_with_rusqlite(&mut conn, |c| {
        let mut writer = MbtilesBulkWriter::new(c, mbt_type)?;
        for chunk in tiles.chunks(100) {
            writer.write_batch(chunk)?;
        }
        Ok(())
    })
    .await
    .unwrap();
    conn
}

async fn dump(conn: &mut SqliteConnection, sql: &str) -> Vec<String> {
    query(AssertSqlSafe(sql))
        .fetch_all(conn)
        .await
        .unwrap()
        .iter()
        .map(|row| {
            (0..row.len())
                .map(|i| {
                    row.try_get::<Vec<u8>, _>(i)
                        .map(|bytes| hex_string(&bytes))
                        .or_else(|_| row.try_get::<i64, _>(i).map(|v| v.to_string()))
                        .or_else(|_| row.try_get::<String, _>(i))
                        .unwrap_or_else(|_| "NULL".to_owned())
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect()
}

fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut hex, b| {
        write!(hex, "{b:02x}").unwrap();
        hex
    })
}

const ALL_TILES: &str =
    "SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles ORDER BY 1, 2, 3";

#[rstest]
#[case::flat(MbtType::Flat)]
#[case::flat_with_hash(MbtType::FlatWithHash)]
#[case::dedup_id(DEDUP_ID)]
#[actix_rt::test]
async fn matches_insert_tiles(#[case] mbt_type: MbtType) {
    let tiles = sample_tiles();
    let mut expected = via_insert_tiles(mbt_type, &tiles).await;
    let mut actual = via_bulk_writer(mbt_type, &tiles).await;

    assert_eq!(
        dump(&mut actual, ALL_TILES).await,
        dump(&mut expected, ALL_TILES).await
    );

    let mbt = Mbtiles::new(":memory:").unwrap();
    let expected_hash = mbt.validate(&mut expected, Full, Update).await.unwrap();
    let actual_hash = mbt.validate(&mut actual, Full, Update).await.unwrap();
    assert_eq!(actual_hash, expected_hash);
}

#[actix_rt::test]
async fn flat_with_hash_stores_same_hashes() {
    let tiles = sample_tiles();
    let sql =
        "SELECT zoom_level, tile_column, tile_row, tile_hash FROM tiles_with_hash ORDER BY 1, 2, 3";
    let mut expected = via_insert_tiles(MbtType::FlatWithHash, &tiles).await;
    let mut actual = via_bulk_writer(MbtType::FlatWithHash, &tiles).await;
    assert_eq!(dump(&mut actual, sql).await, dump(&mut expected, sql).await);
}

#[actix_rt::test]
async fn dedup_id_stores_likely_duplicates_once() {
    let tiles = sample_tiles();
    let sql = "SELECT count(*) FROM tiles_data";
    let mut expected = via_insert_tiles(DEDUP_ID, &tiles).await;
    let mut actual = via_bulk_writer(DEDUP_ID, &tiles).await;
    assert_eq!(dump(&mut actual, sql).await, dump(&mut expected, sql).await);
}

#[actix_rt::test]
async fn dedup_id_unique_hint_skips_lookup() {
    let tile = |x, hint| BulkTile {
        coord: TileCoord::new_unchecked(1, x, 0),
        data: b"same".to_vec(),
        hint,
    };
    let tiles = [tile(0, DedupHint::Unique), tile(1, DedupHint::Unique)];
    let mut conn = via_bulk_writer(DEDUP_ID, &tiles).await;
    assert_eq!(
        dump(&mut conn, "SELECT count(*) FROM tiles_data").await,
        ["2"]
    );
}

#[actix_rt::test]
async fn dedup_id_continues_after_existing_ids() {
    let tiles = sample_tiles();
    let (first, second) = tiles.split_at(tiles.len() / 2);
    let (mbt, mut conn) = new_mbtiles(DEDUP_ID).await;
    for part in [first, second] {
        action_with_rusqlite(&mut conn, |c| {
            MbtilesBulkWriter::new(c, DEDUP_ID)?.write_batch(part)
        })
        .await
        .unwrap();
    }
    let mut expected = via_insert_tiles(DEDUP_ID, &tiles).await;
    assert_eq!(
        dump(&mut conn, ALL_TILES).await,
        dump(&mut expected, ALL_TILES).await
    );
    mbt.validate(&mut conn, Full, Update).await.unwrap();
}

#[actix_rt::test]
async fn failed_batch_is_rolled_back_and_forgotten() {
    let tile = |x, data: &[u8]| BulkTile {
        coord: TileCoord::new_unchecked(2, x, 0),
        data: data.to_vec(),
        hint: DedupHint::LikelyDuplicate,
    };
    let (mbt, mut conn) = new_mbtiles(DEDUP_ID).await;
    action_with_rusqlite(&mut conn, |c| {
        let mut writer = MbtilesBulkWriter::new(c, DEDUP_ID)?;
        writer.write_batch(&[tile(0, b"a")])?;
        let failed = writer.write_batch(&[tile(1, b"b"), tile(1, b"c")]);
        assert_matches!(failed, Err(MbtError::RusqliteError(_)));
        writer.write_batch(&[tile(2, b"b"), tile(3, b"a")])?;
        Ok(())
    })
    .await
    .unwrap();

    assert_eq!(
        dump(&mut conn, ALL_TILES).await,
        ["2,0,3,61", "2,2,3,62", "2,3,3,61"]
    );
    assert_eq!(
        dump(&mut conn, "SELECT count(*) FROM tiles_data").await,
        ["2"]
    );
    mbt.validate(&mut conn, Full, Update).await.unwrap();
}

#[rstest]
#[case::normalized(MbtType::Normalized { hash_view: false, schema: NormalizedSchema::Hash })]
#[case::cache(MbtType::Cache)]
#[actix_rt::test]
async fn rejects_unsupported_types(#[case] mbt_type: MbtType) {
    let (_, mut conn) = new_mbtiles(mbt_type).await;
    let result =
        action_with_rusqlite(&mut conn, |c| MbtilesBulkWriter::new(c, mbt_type).map(drop)).await;
    assert_matches!(result, Err(MbtError::UnsupportedBulkWriteType(t)) if t == mbt_type);
}

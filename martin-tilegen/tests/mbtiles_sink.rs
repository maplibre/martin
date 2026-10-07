#![cfg(feature = "mbtiles")]
#![allow(clippy::unwrap_used)]
use martin_tilegen::{DedupHint, EncodedTile, MbtilesOutput, TileGenError, TileOrder};
use mbtiles::sqlx::{Row as _, query};
use mbtiles::{AggHashType, IntegrityCheckType, MbtType, Mbtiles, NormalizedSchema, TileCoord};
use rstest::rstest;

const DEDUP_ID: MbtType = MbtType::Normalized {
    hash_view: false,
    schema: NormalizedSchema::DedupId,
};

const MAX_ZOOM: u8 = 4;

fn tms_tiles() -> Vec<EncodedTile> {
    let mut tiles: Vec<_> = (0..=MAX_ZOOM)
        .flat_map(|z| (0..1_u32 << z).flat_map(move |x| (0..1_u32 << z).map(move |y| (z, x, y))))
        .map(|(z, x, y)| {
            let (data, hint) = if (x + y) % 3 == 0 {
                (b"ocean".to_vec(), DedupHint::LikelyDuplicate)
            } else {
                (format!("tile {z}/{x}/{y}").into_bytes(), DedupHint::Unique)
            };
            EncodedTile {
                coord: TileCoord::new_unchecked(z, x, y),
                data,
                hint,
            }
        })
        .collect();
    tiles.sort_by_key(|t| TileOrder::Tms.tile_id(t.coord).unwrap());
    tiles
}

fn meta() -> tilejson::TileJSON {
    tilejson::tilejson! { tiles: vec![], minzoom: 0, maxzoom: MAX_ZOOM }
}

#[rstest]
#[tokio::test]
async fn output_validates_and_dedups(
    #[values(MbtType::Flat, MbtType::FlatWithHash, DEDUP_ID)] mbt_type: MbtType,
    #[values(1, 7, 1000)] batch_len: usize,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.mbtiles");
    let tiles = tms_tiles();

    let writer = MbtilesOutput::create(&path, mbt_type)
        .await
        .unwrap()
        .spawn(2, meta())
        .unwrap();
    for (seq, batch) in tiles
        .chunks(batch_len)
        .enumerate()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        writer.submit(seq as u64, batch.to_vec()).unwrap();
    }
    tokio::task::spawn_blocking(move || writer.finish())
        .await
        .unwrap()
        .unwrap();

    let mbt = Mbtiles::new(&path).unwrap();
    let mut conn = mbt.open_readonly().await.unwrap();
    mbt.validate(&mut conn, IntegrityCheckType::Full, AggHashType::Verify)
        .await
        .unwrap();
    assert_eq!(mbt.detect_type(&mut conn).await.unwrap(), mbt_type);

    let count: i64 = query("SELECT count(*) FROM tiles")
        .fetch_one(&mut conn)
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, i64::try_from(tiles.len()).unwrap());
    for tile in &tiles {
        let stored = mbt
            .get_tile(&mut conn, tile.coord.z(), tile.coord.x(), tile.coord.y())
            .await
            .unwrap();
        assert_eq!(
            stored.as_deref(),
            Some(tile.data.as_slice()),
            "{:#}",
            tile.coord
        );
    }
    if mbt_type == DEDUP_ID {
        let stored: i64 = query("SELECT count(*) FROM tiles_data")
            .fetch_one(&mut conn)
            .await
            .unwrap()
            .get(0);
        let ocean = tiles
            .iter()
            .filter(|t| t.hint == DedupHint::LikelyDuplicate)
            .count();
        assert_eq!(stored, i64::try_from(tiles.len() - ocean + 1).unwrap());
    }
}

#[tokio::test]
async fn out_of_order_tiles_fail() {
    let dir = tempfile::tempdir().unwrap();
    let mut tiles = tms_tiles();
    tiles.reverse();
    let writer = MbtilesOutput::create(dir.path().join("out.mbtiles"), DEDUP_ID)
        .await
        .unwrap()
        .spawn(2, meta())
        .unwrap();
    let _ = writer.submit(0, tiles);
    let result = tokio::task::spawn_blocking(move || writer.finish())
        .await
        .unwrap();
    assert!(matches!(result, Err(TileGenError::NotAscending(_))));
}

#[tokio::test]
async fn existing_tiles_are_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.mbtiles");
    let writer = MbtilesOutput::create(&path, DEDUP_ID)
        .await
        .unwrap()
        .spawn(2, meta())
        .unwrap();
    writer.submit(0, tms_tiles()).unwrap();
    tokio::task::spawn_blocking(move || writer.finish())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        MbtilesOutput::create(&path, DEDUP_ID).await,
        Err(TileGenError::OutputNotEmpty(_))
    ));
}

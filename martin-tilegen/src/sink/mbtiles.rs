use std::path::Path;

use mbtiles::sqlx::{AssertSqlSafe, SqliteConnection, raw_sql};
use mbtiles::{
    CopyDuplicateMode, MbtType, Mbtiles, NormalizedSchema, TileDedup, init_mbtiles_schema,
    is_empty_database,
};
use tilejson::TileJSON;
use tokio::runtime::Handle;

use super::{EncodedTile, TileSink};
use crate::{TileGenError, TileGenResult, TileOrder};

const MBT_TYPE: MbtType = MbtType::Normalized {
    hash_view: false,
    schema: NormalizedSchema::DedupId,
};

/// Writes a new deduplicated (`DedupId`) `MBTiles` file through [`Mbtiles::bulk_write`].
/// The mbtiles crate is async, so the sink blocks on the tokio runtime it was created in.
pub struct MbtilesSink {
    runtime: Handle,
    mbt: Mbtiles,
    conn: SqliteConnection,
}

impl MbtilesSink {
    /// Must be called on a blocking thread of a tokio runtime, e.g. inside [`tokio::task::spawn_blocking`].
    pub fn create(path: &Path) -> TileGenResult<Self> {
        let runtime = Handle::current();
        let mbt = Mbtiles::new(path)?;
        let conn = runtime.block_on(async {
            let mut conn = mbt.open_or_new().await?;
            if !is_empty_database(&mut conn).await? {
                return Err(TileGenError::OutputNotEmpty(path.to_path_buf()));
            }
            init_mbtiles_schema(&mut conn, MBT_TYPE, false).await?;
            raw_sql(AssertSqlSafe("PRAGMA page_size = 4096; VACUUM;"))
                .execute(&mut conn)
                .await
                .map_err(mbtiles::MbtError::from)?;
            Ok(conn)
        })?;
        Ok(Self { runtime, mbt, conn })
    }
}

impl TileSink for MbtilesSink {
    fn tile_order(&self) -> TileOrder {
        TileOrder::Tms
    }

    fn write_all(
        &mut self,
        batches: &mut dyn Iterator<Item = TileGenResult<Vec<EncodedTile>>>,
    ) -> TileGenResult<()> {
        let Self { runtime, mbt, conn } = self;
        let write = mbt.bulk_write(conn, MBT_TYPE, CopyDuplicateMode::Abort, |writer| {
            for batch in batches {
                for tile in batch? {
                    let dedup = tile.dedup_key.map_or(TileDedup::Unique, TileDedup::Key);
                    writer.write(tile.coord, &tile.data, dedup)?;
                }
            }
            Ok(())
        });
        runtime.block_on(write)
    }

    fn finish(mut self, metadata: &TileJSON) -> TileGenResult<()> {
        self.runtime.block_on(async {
            self.mbt.insert_metadata(&mut self.conn, metadata).await?;
            self.mbt.update_agg_tiles_hash(&mut self.conn).await?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use martin_tile_utils::TileCoord;
    use mbtiles::{AggHashType, IntegrityCheckType};

    use super::*;

    fn tile(z: u8, x: u32, y: u32, data: &[u8], dedup_key: Option<u64>) -> EncodedTile {
        EncodedTile {
            coord: TileCoord::new_unchecked(z, x, y),
            data: data.to_vec(),
            dedup_key,
        }
    }

    #[tokio::test]
    async fn writes_tiles_and_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.mbtiles");
        tokio::task::spawn_blocking({
            let path = path.clone();
            move || {
                let mut sink = MbtilesSink::create(&path).unwrap();
                let batches = vec![
                    Ok(vec![
                        tile(0, 0, 0, b"root", None),
                        tile(1, 0, 0, b"fill", Some(1)),
                    ]),
                    Ok(vec![
                        tile(1, 0, 1, b"fill", Some(1)),
                        tile(1, 1, 1, b"leaf", None),
                    ]),
                ];
                sink.write_all(&mut batches.into_iter()).unwrap();
                let mut metadata = tilejson::tilejson! { tiles: vec![] };
                metadata.other.insert("format".to_owned(), "pbf".into());
                sink.finish(&metadata).unwrap();
            }
        })
        .await
        .unwrap();

        let mbt = Mbtiles::new(&path).unwrap();
        let mut conn = mbt.open_readonly().await.unwrap();
        mbt.validate(&mut conn, IntegrityCheckType::Full, AggHashType::Verify)
            .await
            .unwrap();
        assert_eq!(
            mbt.get_tile(&mut conn, 1, 0, 1).await.unwrap().unwrap(),
            b"fill"
        );
        assert_eq!(
            mbt.get_tile(&mut conn, 1, 1, 1).await.unwrap().unwrap(),
            b"leaf"
        );
        let blobs: i64 = mbtiles::sqlx::query_scalar("SELECT count(*) FROM tiles_data")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(blobs, 3);
    }

    #[tokio::test]
    async fn upstream_error_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.mbtiles");
        let err = tokio::task::spawn_blocking({
            let path = path.clone();
            move || {
                let mut sink = MbtilesSink::create(&path).unwrap();
                let batches = vec![
                    Ok(vec![tile(0, 0, 0, b"root", None)]),
                    Err(TileGenError::InvalidTileId(3)),
                ];
                sink.write_all(&mut batches.into_iter()).unwrap_err()
            }
        })
        .await
        .unwrap();
        assert!(matches!(err, TileGenError::InvalidTileId(3)));

        let mbt = Mbtiles::new(&path).unwrap();
        let mut conn = mbt.open_readonly().await.unwrap();
        let tiles: i64 = mbtiles::sqlx::query_scalar("SELECT count(*) FROM tiles")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(tiles, 0);
    }

    #[tokio::test]
    async fn refuses_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.mbtiles");
        let second = tokio::task::spawn_blocking(move || {
            drop(MbtilesSink::create(&path).unwrap());
            MbtilesSink::create(&path)
        })
        .await
        .unwrap();
        assert!(matches!(second, Err(TileGenError::OutputNotEmpty(_))));
    }
}

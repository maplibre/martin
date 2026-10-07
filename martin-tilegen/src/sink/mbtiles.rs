use std::path::Path;

use mbtiles::sqlx::{AssertSqlSafe, SqliteConnection, raw_sql};
use mbtiles::{
    CopyDuplicateMode, MbtType, Mbtiles, NormalizedSchema, TileDedup, init_mbtiles_schema,
    is_empty_database,
};
use tilejson::TileJSON;
use tokio::runtime::Runtime;

use super::{EncodedTile, TileSink};
use crate::{TileGenError, TileGenResult, TileOrder};

const MBT_TYPE: MbtType = MbtType::Normalized {
    hash_view: false,
    schema: NormalizedSchema::DedupId,
};

/// Writes a new deduplicated (`DedupId`) `MBTiles` file through [`Mbtiles::bulk_write`].
/// The mbtiles crate is async; this sink owns a single-threaded runtime because it runs on the writer thread.
pub struct MbtilesSink {
    runtime: Runtime,
    mbt: Mbtiles,
    conn: SqliteConnection,
}

impl MbtilesSink {
    pub fn create(path: &Path) -> TileGenResult<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let mbt = Mbtiles::new(path)?;
        let conn = runtime.block_on(async {
            let mut conn = mbt.open_or_new().await?;
            if !is_empty_database(&mut conn).await? {
                return Err(TileGenError::OutputNotEmpty(path.to_path_buf()));
            }
            init_mbtiles_schema(&mut conn, MBT_TYPE, false).await?;
            // 4 KiB pages instead of the schema default 512 B take ~30% fewer cycles to bulk insert.
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
        // An upstream error rolls back the tiles written since the last periodic commit.
        let write = mbt.bulk_write(conn, MBT_TYPE, CopyDuplicateMode::Abort, |writer| {
            for batch in batches {
                for tile in batch? {
                    // The engine verified that tiles with the same key have the same bytes.
                    let dedup = tile.dedup.map_or(TileDedup::Unique, TileDedup::Key);
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

    fn tile(z: u8, x: u32, y: u32, data: &[u8], dedup: Option<u64>) -> EncodedTile {
        EncodedTile {
            coord: TileCoord::new_unchecked(z, x, y),
            data: data.to_vec(),
            dedup,
        }
    }

    #[test]
    fn writes_tiles_and_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.mbtiles");
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

        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(async {
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
        });
    }

    #[test]
    fn upstream_error_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.mbtiles");
        let mut sink = MbtilesSink::create(&path).unwrap();
        let batches = vec![
            Ok(vec![tile(0, 0, 0, b"root", None)]),
            Err(TileGenError::InvalidTileId(3)),
        ];
        let err = sink.write_all(&mut batches.into_iter()).unwrap_err();
        assert!(matches!(err, TileGenError::InvalidTileId(3)));
    }

    #[test]
    fn refuses_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.mbtiles");
        drop(MbtilesSink::create(&path).unwrap());
        assert!(matches!(
            MbtilesSink::create(&path),
            Err(TileGenError::OutputNotEmpty(_))
        ));
    }
}

use std::path::Path;

use mbtiles::sqlx::{Connection as _, SqliteConnection};
use mbtiles::{
    BulkTile, MbtType, Mbtiles, MbtilesBulkWriter, action_with_rusqlite, init_mbtiles_schema,
    is_empty_database,
};
use tilejson::TileJSON;

use crate::{DedupHint, EncodedTile, OrderedWriter, TileGenError, TileGenResult, TileOrder};

/// A new `MBTiles` file ready to receive tiles in [`TileOrder::Tms`].
pub struct MbtilesOutput {
    mbt: Mbtiles,
    conn: SqliteConnection,
    mbt_type: MbtType,
}

impl MbtilesOutput {
    /// Creates the schema of `mbt_type`, which is one of the flat types or the `DedupId` normalized schema.
    pub async fn create(path: impl AsRef<Path>, mbt_type: MbtType) -> TileGenResult<Self> {
        let path = path.as_ref();
        let mbt = Mbtiles::new(path)?;
        let mut conn = mbt.open_or_new().await?;
        if !is_empty_database(&mut conn).await? {
            return Err(TileGenError::OutputNotEmpty(path.to_path_buf()));
        }
        init_mbtiles_schema(&mut conn, mbt_type, false).await?;
        Ok(Self {
            mbt,
            conn,
            mbt_type,
        })
    }

    /// Starts the writer thread. It writes the whole tile stream inside one
    /// [`action_with_rusqlite`] call, then stores `meta` and the aggregate tiles hash.
    pub fn spawn(self, capacity: usize, meta: TileJSON) -> TileGenResult<OrderedWriter> {
        let Self {
            mbt,
            mut conn,
            mbt_type,
        } = self;
        OrderedWriter::spawn(TileOrder::Tms, capacity, move |mut batches| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(async move {
                let mut ordering_failure = None;
                action_with_rusqlite(&mut conn, |rusqlite| {
                    let mut writer = MbtilesBulkWriter::new(rusqlite, mbt_type)?;
                    loop {
                        match batches.next_batch() {
                            Ok(Some(batch)) => writer.write_batch(&bulk_tiles(batch))?,
                            Ok(None) => break,
                            Err(e) => {
                                ordering_failure = Some(e);
                                break;
                            }
                        }
                    }
                    Ok(())
                })
                .await?;
                if let Some(failure) = ordering_failure {
                    return Err(failure);
                }
                mbt.insert_metadata(&mut conn, &meta).await?;
                mbt.update_agg_tiles_hash(&mut conn).await?;
                conn.close().await.map_err(mbtiles::MbtError::from)?;
                Ok(())
            })
        })
    }
}

fn bulk_tiles(batch: Vec<EncodedTile>) -> Vec<BulkTile<Vec<u8>>> {
    batch
        .into_iter()
        .map(|tile| BulkTile {
            coord: tile.coord,
            data: tile.data,
            hint: match tile.hint {
                DedupHint::Unique => mbtiles::DedupHint::Unique,
                DedupHint::LikelyDuplicate => mbtiles::DedupHint::LikelyDuplicate,
            },
        })
        .collect()
}

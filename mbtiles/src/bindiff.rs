use std::future::Future;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::Instant;

use flume::{bounded, Receiver, Sender};
use futures::TryStreamExt;
use log::{debug, error, info};
use martin_tile_utils::TileCoord;
use sqlite_compressions::{BsdiffRawDiffer, Differ as _, Encoder as _, GzipEncoder};
use sqlx::{query, Executor, Row, SqliteConnection};

use crate::MbtType::{Flat, FlatWithHash, Normalized};
use crate::{create_bsdiffraw_tables, MbtError, MbtResult, MbtType, Mbtiles};

pub trait BinDiffer<S: Send + 'static, T: Send + 'static>: Sized + Send + Sync + 'static {
    fn query(
        &self,
        sql_where: String,
        tx_wrk: Sender<S>,
    ) -> impl Future<Output = MbtResult<()>> + Send;

    fn process(&self, value: S) -> MbtResult<T>;

    fn insert(
        &self,
        value: T,
        conn: &mut SqliteConnection,
    ) -> impl Future<Output = MbtResult<()>> + Send;

    async fn run(self, conn: &mut SqliteConnection, sql_where: String) -> MbtResult<()> {
        let patcher = Arc::new(self);
        let has_errors = Arc::new(AtomicBool::new(false));
        let (tx_wrk, rx_wrk) = bounded(num_cpus::get() * 3);
        let (tx_ins, rx_ins) = bounded::<T>(num_cpus::get() * 3);

        {
            let has_errors = has_errors.clone();
            let patcher = patcher.clone();
            tokio::spawn(async move {
                if let Err(e) = patcher.query(sql_where, tx_wrk).await {
                    error!("Failed to query bindiff data: {e}");
                    has_errors.store(true, Relaxed);
                }
            });
        }

        start_processor_threads(patcher.clone(), rx_wrk, tx_ins, has_errors.clone());
        recv_and_insert(patcher, conn, rx_ins).await?;

        if has_errors.load(Relaxed) {
            Err(MbtError::BindiffError)
        } else {
            Ok(())
        }
    }
}

async fn recv_and_insert<S: Send + 'static, T: Send + 'static, P: BinDiffer<S, T>>(
    patcher: Arc<P>,
    conn: &mut SqliteConnection,
    rx_ins: Receiver<T>,
) -> MbtResult<()> {
    create_bsdiffraw_tables(&mut *conn).await?;
    conn.execute("BEGIN").await?;
    let mut inserted = 0;
    let mut last_report_ts = Instant::now();
    while let Ok(res) = rx_ins.recv_async().await {
        patcher.insert(res, conn).await?;
        inserted += 1;
        if inserted % 100 == 0 {
            conn.execute("COMMIT").await?;
            if last_report_ts.elapsed().as_secs() >= 10 {
                info!("Processed {inserted} bindiff tiles");
                last_report_ts = Instant::now();
            }
            conn.execute("BEGIN").await?;
        }
    }
    conn.execute("COMMIT").await?;
    info!("Finished processing {inserted} bindiff tiles");

    Ok(())
}

// Both tx and rcv must be consumed, or it will run forever
#[allow(clippy::needless_pass_by_value)]
fn start_processor_threads<S: Send + 'static, T: Send + 'static, P: BinDiffer<S, T>>(
    patcher: Arc<P>,
    rx_wrk: Receiver<S>,
    tx_ins: Sender<T>,
    has_errors: Arc<AtomicBool>,
) {
    (0..num_cpus::get()).for_each(|_| {
        let rx_wrk = rx_wrk.clone();
        let tx_ins = tx_ins.clone();
        let has_errors = has_errors.clone();
        let patcher = patcher.clone();
        tokio::spawn(async move {
            while let Ok(wrk) = rx_wrk.recv_async().await {
                if match patcher.process(wrk) {
                    Ok(res) => tx_ins.send_async(res).await.is_err(),
                    Err(..) => true,
                } {
                    has_errors.store(true, Relaxed);
                }
                // also stop processing if another processor stopped
                if has_errors.load(Relaxed) {
                    break;
                }
            }
        });
    });
}

pub struct DifferBefore {
    coord: TileCoord,
    old_tile_data: Vec<u8>,
    new_tile_data: Vec<u8>,
}

pub struct DifferAfter {
    coord: TileCoord,
    data: Vec<u8>,
    new_tile_hash: String,
}

pub struct BinDiffDiffer {
    src_mbt: Mbtiles,
    dif_mbt: Mbtiles,
    dif_type: MbtType,
}

impl BinDiffDiffer {
    pub fn new(src_mbt: Mbtiles, dif_mbt: Mbtiles, dif_type: MbtType) -> Self {
        Self {
            src_mbt,
            dif_mbt,
            dif_type,
        }
    }
}

impl BinDiffer<DifferBefore, DifferAfter> for BinDiffDiffer {
    async fn query(&self, sql_where: String, tx_wrk: Sender<DifferBefore>) -> MbtResult<()> {
        let diff_tiles = match self.dif_type {
            Flat => "diffDb.tiles",
            FlatWithHash => "diffDb.tiles_with_hash",
            Normalized { .. } => {
                "
        (SELECT zoom_level, tile_column, tile_row, tile_data, map.tile_id AS tile_hash
        FROM diffDb.map JOIN diffDb.images ON diffDb.map.tile_id = diffDb.images.tile_id)"
            }
        };

        let sql = format!(
            "
        SELECT srcTiles.zoom_level
             , srcTiles.tile_column
             , srcTiles.tile_row
             , srcTiles.tile_data old_tile_data
             , difTiles.tile_data new_tile_data
        FROM tiles AS srcTiles JOIN {diff_tiles} AS difTiles
             ON srcTiles.zoom_level = difTiles.zoom_level
               AND srcTiles.tile_column = difTiles.tile_column
               AND srcTiles.tile_row = difTiles.tile_row
        WHERE srcTiles.tile_data != difTiles.tile_data {sql_where}"
        );

        let mut conn = self.src_mbt.open_readonly().await?;
        self.dif_mbt.attach_to(&mut conn, "diffDb").await?;
        debug!("Querying bsdiffraw data with {sql}");
        let mut rows = query(&sql).fetch(&mut conn);

        while let Some(row) = rows.try_next().await? {
            let work = DifferBefore {
                coord: TileCoord {
                    z: row.get(0),
                    x: row.get(1),
                    y: row.get(2),
                },
                old_tile_data: row.get(3),
                new_tile_data: row.get(4),
            };
            if tx_wrk.send_async(work).await.is_err() {
                break; // the receiver has been dropped
            }
        }

        Ok(())
    }

    fn process(&self, value: DifferBefore) -> MbtResult<DifferAfter> {
        let old_tile = GzipEncoder::decode(&value.old_tile_data)
            .inspect_err(|e| error!("Unable to unzip source tile at {:?}: {e}", value.coord))?;
        let new_tile = GzipEncoder::decode(&value.new_tile_data)
            .inspect_err(|e| error!("Unable to unzip diff tile at {:?}: {e}", value.coord))?;
        let diff = BsdiffRawDiffer::diff(&old_tile, &new_tile).expect("BinDiff failure");

        Ok(DifferAfter {
            coord: value.coord,
            data: diff,
            new_tile_hash: format!("{:X}", md5::compute(&new_tile)),
        })
    }

    async fn insert(&self, value: DifferAfter, conn: &mut SqliteConnection) -> MbtResult<()> {
        query("INSERT INTO bsdiffraw (zoom_level, tile_column, tile_row, patch_data, uncompressed_tile_hash) VALUES (?, ?, ?, ?, ?)")
            .bind(value.coord.z)
            .bind(value.coord.x)
            .bind(value.coord.y)
            .bind(value.data)
            .bind(value.new_tile_hash)
            .execute(&mut *conn).await?;
        Ok(())
    }
}

pub struct ApplierBefore {
    coord: TileCoord,
    tile_data: Vec<u8>,
    patch_data: Vec<u8>,
    uncompressed_tile_hash: String,
}

pub struct ApplierAfter {
    coord: TileCoord,
    data: Vec<u8>,
    new_tile_hash: String,
}

pub struct BinDiffPatcher {
    src_mbt: Mbtiles,
    dif_mbt: Mbtiles,
    /// Whether the bindiff table has the `uncompressed_tile_hash` column for validation
    diff_has_hash: bool,
    /// Whether we insert into the `tiles_with_hash` table or the `tiles` table
    target_has_hash: bool,
}

impl BinDiffPatcher {
    pub fn new(
        src_mbt: Mbtiles,
        dif_mbt: Mbtiles,
        diff_has_hash: bool,
        target_has_hash: bool,
    ) -> Self {
        Self {
            src_mbt,
            dif_mbt,
            diff_has_hash,
            target_has_hash,
        }
    }
}

impl BinDiffer<ApplierBefore, ApplierAfter> for BinDiffPatcher {
    async fn query(&self, sql_where: String, tx_wrk: Sender<ApplierBefore>) -> MbtResult<()> {
        let get_uncompressed_tile_hash = if self.diff_has_hash {
            ", uncompressed_tile_hash"
        } else {
            ""
        };

        let sql = format!(
            "
        SELECT srcTiles.zoom_level
             , srcTiles.tile_column
             , srcTiles.tile_row
             , srcTiles.tile_data
             , patch_data
             {get_uncompressed_tile_hash}
        FROM tiles AS srcTiles JOIN diffDb.bsdiffraw AS difTiles
             ON srcTiles.zoom_level = difTiles.zoom_level
               AND srcTiles.tile_column = difTiles.tile_column
               AND srcTiles.tile_row = difTiles.tile_row
        WHERE TRUE {sql_where}"
        );

        let mut conn = self.src_mbt.open_readonly().await?;
        self.dif_mbt.attach_to(&mut conn, "diffDb").await?;
        debug!("Querying bsdiffraw data with {sql}");
        let mut rows = query(&sql).fetch(&mut conn);

        while let Some(row) = rows.try_next().await? {
            let work = ApplierBefore {
                coord: TileCoord {
                    z: row.get(0),
                    x: row.get(1),
                    y: row.get(2),
                },
                tile_data: row.get(3),
                patch_data: row.get(4),
                uncompressed_tile_hash: self.diff_has_hash.then(|| row.get(5)).unwrap_or_default(),
            };
            if tx_wrk.send_async(work).await.is_err() {
                break; // the receiver has been dropped
            }
        }

        Ok(())
    }

    fn process(&self, value: ApplierBefore) -> MbtResult<ApplierAfter> {
        let tile_data = GzipEncoder::decode(&value.tile_data)
            .inspect_err(|e| error!("Unable to unzip source tile at {:?}: {e}", value.coord))?;
        let new_tile = BsdiffRawDiffer::patch(&tile_data, &value.patch_data)?;

        if self.diff_has_hash {
            let new_tile_hash = format!("{:X}", md5::compute(&new_tile));
            if new_tile_hash != value.uncompressed_tile_hash {
                return Err(MbtError::BinDiffIncorrectTileHash(
                    value.coord.to_string(),
                    value.uncompressed_tile_hash,
                    new_tile_hash,
                ));
            }
        }

        let data = GzipEncoder::encode(&new_tile, Some(9))?;

        Ok(ApplierAfter {
            coord: value.coord,
            new_tile_hash: self
                .target_has_hash
                .then(|| format!("{:X}", md5::compute(&data)))
                .unwrap_or_default(),
            data,
        })
    }

    async fn insert(&self, value: ApplierAfter, conn: &mut SqliteConnection) -> MbtResult<()> {
        let mut q = query(
            if self.target_has_hash {
                "INSERT INTO tiles_with_hash (zoom_level, tile_column, tile_row, tile_data, tile_hash) VALUES (?, ?, ?, ?, ?)"
            } else {
                "INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES (?, ?, ?, ?)"
            })
        .bind(value.coord.z)
        .bind(value.coord.x)
        .bind(value.coord.y)
        .bind(value.data);
        if self.target_has_hash {
            q = q.bind(value.new_tile_hash);
        }

        q.execute(&mut *conn).await?;
        Ok(())
    }
}

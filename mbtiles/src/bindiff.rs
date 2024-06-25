use std::future::Future;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::Instant;

use flume::{bounded, Receiver, Sender};
use futures::TryStreamExt;
use log::{debug, error, info};
use martin_tile_utils::{decode_brotli, decode_gzip, encode_brotli, encode_gzip, TileCoord};
use sqlite_compressions::{BsdiffRawDiffer, Differ as _};
use sqlx::{query, Executor, Row, SqliteConnection};
use xxhash_rust::xxh3::xxh3_64;

use crate::MbtType::{Flat, FlatWithHash, Normalized};
use crate::PatchType::Whole;
use crate::{
    create_bsdiffraw_tables, get_bsdiff_tbl_name, MbtError, MbtResult, MbtType, Mbtiles, PatchType,
};

pub trait BinDiffer<S: Send + 'static, T: Send + 'static>: Sized + Send + Sync + 'static {
    fn query(
        &self,
        sql_where: String,
        tx_wrk: Sender<S>,
    ) -> impl Future<Output = MbtResult<()>> + Send;

    fn process(&self, value: S) -> MbtResult<T>;

    fn before_insert(
        &self,
        conn: &mut SqliteConnection,
    ) -> impl Future<Output = MbtResult<()>> + Send;

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
    patcher.before_insert(conn).await?;
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
                    Err(e) => {
                        error!("Failed to process bindiff data: {e}");
                        true
                    }
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
    new_tile_hash: u64,
}

pub struct BinDiffDiffer {
    src_mbt: Mbtiles,
    dif_mbt: Mbtiles,
    dif_type: MbtType,
    patch_type: PatchType,
    insert_sql: String,
}

impl BinDiffDiffer {
    pub fn new(
        src_mbt: Mbtiles,
        dif_mbt: Mbtiles,
        dif_type: MbtType,
        patch_type: PatchType,
    ) -> Self {
        assert_ne!(patch_type, Whole, "Invalid for BinDiffDiffer");
        let insert_sql = format!(
            "INSERT INTO {}(zoom_level, tile_column, tile_row, patch_data, tile_xxh3_64_hash) VALUES (?, ?, ?, ?, ?)",
            get_bsdiff_tbl_name(patch_type));
        Self {
            src_mbt,
            dif_mbt,
            dif_type,
            patch_type,
            insert_sql,
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
        debug!("Querying source data with {sql}");
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
        let mut old_tile = value.old_tile_data;
        let mut new_tile = value.new_tile_data;
        if self.patch_type == PatchType::BinDiffGz {
            old_tile = decode_gzip(&old_tile).inspect_err(|e| {
                error!("Unable to gzip-decode source tile {:?}: {e}", value.coord);
            })?;
            new_tile = decode_gzip(&new_tile).inspect_err(|e| {
                error!("Unable to gzip-decode diff tile {:?}: {e}", value.coord);
            })?;
        }
        let new_tile_hash = xxh3_64(&new_tile);
        let data = BsdiffRawDiffer::diff(&old_tile, &new_tile).expect("BinDiff failure");
        let data = encode_brotli(&data)?;

        Ok(DifferAfter {
            coord: value.coord,
            data,
            new_tile_hash,
        })
    }

    async fn before_insert(&self, conn: &mut SqliteConnection) -> MbtResult<()> {
        create_bsdiffraw_tables(conn, self.patch_type).await
    }

    async fn insert(&self, value: DifferAfter, conn: &mut SqliteConnection) -> MbtResult<()> {
        #[allow(clippy::cast_possible_wrap)]
        query(self.insert_sql.as_str())
            .bind(value.coord.z)
            .bind(value.coord.x)
            .bind(value.coord.y)
            .bind(value.data)
            .bind(value.new_tile_hash as i64)
            .execute(&mut *conn)
            .await?;
        Ok(())
    }
}

pub struct ApplierBefore {
    coord: TileCoord,
    tile_data: Vec<u8>,
    patch_data: Vec<u8>,
    uncompressed_tile_hash: u64,
}

pub struct ApplierAfter {
    coord: TileCoord,
    data: Vec<u8>,
    new_tile_hash: String,
}

pub struct BinDiffPatcher {
    src_mbt: Mbtiles,
    dif_mbt: Mbtiles,
    dst_type: MbtType,
    patch_type: PatchType,
}

impl BinDiffPatcher {
    pub fn new(
        src_mbt: Mbtiles,
        dif_mbt: Mbtiles,
        dst_type: MbtType,
        patch_type: PatchType,
    ) -> Self {
        Self {
            src_mbt,
            dif_mbt,
            dst_type,
            patch_type,
        }
    }
}

impl BinDiffer<ApplierBefore, ApplierAfter> for BinDiffPatcher {
    async fn query(&self, sql_where: String, tx_wrk: Sender<ApplierBefore>) -> MbtResult<()> {
        let tbl = get_bsdiff_tbl_name(self.patch_type);
        let sql = format!(
            "
        SELECT srcTiles.zoom_level
             , srcTiles.tile_column
             , srcTiles.tile_row
             , srcTiles.tile_data
             , patch_data
             , tile_xxh3_64_hash
        FROM tiles AS srcTiles JOIN diffDb.{tbl} AS difTiles
             ON srcTiles.zoom_level = difTiles.zoom_level
               AND srcTiles.tile_column = difTiles.tile_column
               AND srcTiles.tile_row = difTiles.tile_row
        WHERE TRUE {sql_where}"
        );

        let mut conn = self.src_mbt.open_readonly().await?;
        self.dif_mbt.attach_to(&mut conn, "diffDb").await?;
        debug!("Querying {tbl} table with {sql}");
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
                #[allow(clippy::cast_sign_loss)]
                uncompressed_tile_hash: row.get::<i64, _>(5) as u64,
            };
            if tx_wrk.send_async(work).await.is_err() {
                break; // the receiver has been dropped
            }
        }

        Ok(())
    }

    fn process(&self, value: ApplierBefore) -> MbtResult<ApplierAfter> {
        let tile_data = decode_gzip(&value.tile_data)
            .inspect_err(|e| error!("Unable to gzip-decode source tile {:?}: {e}", value.coord))?;
        let patch_data = decode_brotli(&value.patch_data)
            .inspect_err(|e| error!("Unable to brotli-decode patch data {:?}: {e}", value.coord))?;
        let new_tile = BsdiffRawDiffer::patch(&tile_data, &patch_data)
            .inspect_err(|e| error!("Unable to patch tile {:?}: {e}", value.coord))?;
        let new_tile_hash = xxh3_64(&new_tile);
        if new_tile_hash != value.uncompressed_tile_hash {
            return Err(MbtError::BinDiffIncorrectTileHash(
                value.coord.to_string(),
                value.uncompressed_tile_hash.to_string(),
                new_tile_hash.to_string(),
            ));
        }

        let data = encode_gzip(&new_tile)?;

        Ok(ApplierAfter {
            coord: value.coord,
            new_tile_hash: if self.dst_type == FlatWithHash {
                format!("{:X}", md5::compute(&data))
            } else {
                String::default() // This is a fast noop, no memory alloc is performed
            },
            data,
        })
    }

    async fn before_insert(&self, _conn: &mut SqliteConnection) -> MbtResult<()> {
        Ok(())
    }

    async fn insert(&self, value: ApplierAfter, conn: &mut SqliteConnection) -> MbtResult<()> {
        let mut q = query(
            match self.dst_type {
                Flat =>"INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES (?, ?, ?, ?)",
                FlatWithHash => "INSERT INTO tiles_with_hash (zoom_level, tile_column, tile_row, tile_data, tile_hash) VALUES (?, ?, ?, ?, ?)",
                v @ Normalized { .. } => return Err(MbtError::BinDiffRequiresFlatWithHash(v)),
            })
        .bind(value.coord.z)
        .bind(value.coord.x)
        .bind(value.coord.y)
        .bind(value.data);

        if self.dst_type == FlatWithHash {
            q = q.bind(value.new_tile_hash);
        }

        q.execute(&mut *conn).await?;
        Ok(())
    }
}

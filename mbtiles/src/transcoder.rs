use std::mem;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use flume::{Receiver, Sender, bounded};
use futures::TryStreamExt as _;
use log::{debug, info, warn};
use moka::sync::Cache;
use rayon::iter::{IntoParallelIterator as _, ParallelIterator as _};
use sqlx::{Connection as _, Row as _, SqliteConnection};
use tokio::task::spawn_blocking;
use xxhash_rust::xxh3::xxh3_128;

use crate::errors::MbtResult;
use crate::mbtiles::parse_tile_index;
use crate::queries::{detach_db, init_mbtiles_schema};
use crate::{CopyDuplicateMode, MbtError, MbtType, Mbtiles, TileCoord};

/// Default number of tiles per batch in the pipeline.
const DEFAULT_BATCH_SIZE: usize = 500;

/// Default maximum tile size (bytes) for dedup cache tracking.
/// Only small tiles (empty ocean, backgrounds) tend to repeat.
const DEFAULT_MAX_TILE_TRACK_SIZE: usize = 1024;

/// Default maximum cache weight in bytes (512 MiB).
const DEFAULT_CACHE_MAX_BYTES: u64 = 512 * 1024 * 1024;

/// Default channel buffer depth (backpressure).
const DEFAULT_CHANNEL_BUFFER: usize = 4;

/// Maximum time between forced flushes in the writer.
const FLUSH_INTERVAL: Duration = Duration::from_secs(60);

/// Raw tile batch: `(coord, optional_cache_key, tile_data)`.
type RawBatch = Vec<(TileCoord, Option<u128>, Vec<u8>)>;
/// Encoded tile batch: `(coord, encoded_data)`.
/// Uses `Arc` to avoid cloning tile bytes on dedup cache hits.
type EncodedBatch = Vec<(TileCoord, Arc<Vec<u8>>)>;

/// Normalized tiles batch: `(tile_id_string, tile_data)`.
type NormRawBatch = Vec<(String, Vec<u8>)>;
/// Normalized encoded batch: `(tile_id_string, encoded_data)`.
type NormEncBatch = Vec<(String, Vec<u8>)>;

/// Weighted dedup cache: maps content hash -> encoded tile bytes.
type EncodedCache = Cache<u128, Arc<Vec<u8>>>;

/// Statistics returned after transcoding completes.
#[derive(Debug, Clone)]
pub struct TranscodeStats {
    pub tiles_written: usize,
    pub cache_hits: u64,
    pub cache_encoded: u64,
}

/// Internal atomic counters for the dedup cache.
#[derive(Default)]
struct DedupStats {
    hits: AtomicU64,
    encoded: AtomicU64,
}

impl DedupStats {
    fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }
    fn record_encode(&self) {
        self.encoded.fetch_add(1, Ordering::Relaxed);
    }
}

/// Builder for a parallelized mbtiles-to-mbtiles transcoding pipeline.
///
/// The transform closure is applied to every unique tile payload. The pipeline
/// automatically selects the most efficient strategy based on source/destination
/// schema types:
///
/// - **Normalized source**: encodes only the deduplicated `tiles` table, then
///   fans out to any destination type via SQL joins.
/// - **Flat/FlatWithHash source**: uses a weighted dedup cache keyed by content
///   hash to avoid redundant transforms.
///
/// CPU-bound work runs on a rayon thread pool via [`tokio::task::spawn_blocking`].
///
/// # Example
/// ```no_run
/// # async fn example() -> mbtiles::MbtResult<()> {
/// use std::path::PathBuf;
/// use mbtiles::MbtilesTranscoder;
///
/// let stats = MbtilesTranscoder::new(
///     PathBuf::from("input.mbtiles"),
///     PathBuf::from("output.mbtiles"),
///     |data| Ok(data), // identity transform
/// )
/// .batch_size(1000)
/// .run()
/// .await?;
///
/// println!("Wrote {} tiles", stats.tiles_written);
/// # Ok(())
/// # }
/// ```
pub struct MbtilesTranscoder<F> {
    src_file: PathBuf,
    dst_file: PathBuf,
    transform: F,
    dst_type: Option<MbtType>,
    batch_size: usize,
    cache_max_bytes: u64,
    max_tile_track_size: usize,
    copy_metadata: bool,
    channel_buffer: usize,
}

impl<F> MbtilesTranscoder<F>
where
    F: Fn(Vec<u8>) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>
        + Send
        + Sync
        + 'static,
{
    /// Create a new transcoder with required parameters and sensible defaults.
    pub fn new(src_file: PathBuf, dst_file: PathBuf, transform: F) -> Self {
        Self {
            src_file,
            dst_file,
            transform,
            dst_type: None,
            batch_size: DEFAULT_BATCH_SIZE,
            cache_max_bytes: DEFAULT_CACHE_MAX_BYTES,
            max_tile_track_size: DEFAULT_MAX_TILE_TRACK_SIZE,
            copy_metadata: true,
            channel_buffer: DEFAULT_CHANNEL_BUFFER,
        }
    }

    /// Set the destination schema type. If not set, inherits from source.
    #[must_use]
    pub fn dst_type(mut self, dst_type: MbtType) -> Self {
        self.dst_type = Some(dst_type);
        self
    }

    /// Set the number of tiles per batch. Default: 500.
    #[must_use]
    pub fn batch_size(mut self, n: usize) -> Self {
        self.batch_size = n;
        self
    }

    /// Set maximum cache weight in bytes. Default: 512 MiB.
    #[must_use]
    pub fn cache_max_bytes(mut self, n: u64) -> Self {
        self.cache_max_bytes = n;
        self
    }

    /// Set the maximum tile size (bytes) to track in the dedup cache. Default: 1024.
    #[must_use]
    pub fn max_tile_track_size(mut self, n: usize) -> Self {
        self.max_tile_track_size = n;
        self
    }

    /// Whether to copy metadata from source to destination. Default: true.
    #[must_use]
    pub fn copy_metadata(mut self, v: bool) -> Self {
        self.copy_metadata = v;
        self
    }

    /// Set the channel buffer depth for backpressure. Default: 4.
    #[must_use]
    pub fn channel_buffer(mut self, n: usize) -> Self {
        self.channel_buffer = n;
        self
    }

    /// Run the transcoding pipeline.
    pub async fn run(self) -> MbtResult<TranscodeStats> {
        let src = Mbtiles::new(&self.src_file)?;
        let mut src_conn = src.open_readonly().await?;
        let src_type = src.detect_type(&mut src_conn).await?;
        let dst_type = self.dst_type.unwrap_or(src_type);

        let dst = Mbtiles::new(&self.dst_file)?;
        let mut dst_conn = dst.open_or_new().await?;
        init_mbtiles_schema(&mut dst_conn, dst_type).await?;

        // Set WAL mode for better write throughput.
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&mut *dst_conn)
            .await?;
        sqlx::query("PRAGMA synchronous=NORMAL")
            .execute(&mut *dst_conn)
            .await?;

        if self.copy_metadata {
            copy_metadata(&src, &mut dst_conn).await?;
        }

        info!("Transcoding {src} ({src_type}) to {dst} ({dst_type})");

        let stats = if let Some(src_type) = src_type.normalized_schema() {
            self.run_normalized_path(src, &mut src_conn, src_type, &mut dst_conn, dst_type)
                .await?
        } else {
            self.run_general_path(src_conn, src_type, dst, dst_conn, dst_type)
                .await?
        };

        Ok(stats)
    }

    /// Normalized -> Any: encode only unique tiles, then fan out via SQL.
    async fn run_normalized_path(
        self,
        src: Mbtiles,
        src_conn: &mut SqliteConnection,
        src_schema: NormalizedSchema,
        dst_conn: &mut SqliteConnection,
        dst_type: MbtType,
    ) -> MbtResult<TranscodeStats> {
        let tile_id_col = src_schema.tile_id_column();
        let src_map = src_schema.map_table();

        // Phase 1: Stream-encode unique tiles via the 3-stage pipeline.
        let (raw_tx, raw_rx) = bounded::<NormRawBatch>(self.channel_buffer);
        let (enc_tx, enc_rx) = bounded::<NormEncBatch>(self.channel_buffer);

        let batch_size = self.batch_size;
        let transform = Arc::new(self.transform);

        let sql = format!(
            "SELECT {tile_id_col}, tile_data FROM {content_table}",
            content_table = src_schema.content_table()
        );
        let reader = normalized_reader(src_conn, &sql, raw_tx, batch_size);

        let compute = normalized_compute(raw_rx, enc_tx, transform);

        let writer = normalized_writer(dst_conn, enc_rx);

        let ((), (), tiles_written) = tokio::try_join!(reader, compute, writer)?;

        info!("Encoded {tiles_written} unique tiles");

        src.attach_to(&mut *dst_conn, "srcDb").await?;

        match dst_type {
            MbtType::Normalized { .. } => {
                let dst_schema = dst_type.normalized_schema().expect("dst is normalized");
                let dst_tiles = dst_schema.content_table();
                let dst_id = dst_schema.tile_id_column();
                let dst_map = dst_schema.map_table();

                sqlx::query(&format!(
                    "INSERT OR REPLACE INTO {dst_tiles} ({dst_id}, tile_data)
                     SELECT tile_id, tile_data FROM _transcoded_tiles"
                ))
                .execute(&mut *dst_conn)
                .await?;

                sqlx::query(&format!(
                    "INSERT OR REPLACE INTO {dst_map} (zoom_level, tile_column, tile_row, {dst_id})
                     SELECT zoom_level, tile_column, tile_row, {tile_id_col}
                     FROM srcDb.{src_map}"
                ))
                .execute(&mut *dst_conn)
                .await?;
            }
            MbtType::Flat => {
                sqlx::query(&format!(
                    "INSERT OR REPLACE INTO tiles (zoom_level, tile_column, tile_row, tile_data)
                     SELECT m.zoom_level, m.tile_column, m.tile_row, t.tile_data
                     FROM srcDb.{src_map} m
                     JOIN _transcoded_tiles t ON m.{tile_id_col} = t.tile_id"
                ))
                .execute(&mut *dst_conn)
                .await?;
            }
            MbtType::FlatWithHash => {
                sqlx::query(&format!(
                    "INSERT OR REPLACE INTO tiles_with_hash (zoom_level, tile_column, tile_row, tile_data, tile_hash)
                     SELECT m.zoom_level, m.tile_column, m.tile_row, t.tile_data, md5_hex(t.tile_data)
                     FROM srcDb.{src_map} m
                     JOIN _transcoded_tiles t ON m.{tile_id_col} = t.tile_id"
                ))
                .execute(&mut *dst_conn)
                .await?;
            }
        }

        let tiles_written: i64 =
            sqlx::query_scalar(&format!("SELECT COUNT(*) FROM srcDb.{src_map}"))
                .fetch_one(&mut *dst_conn)
                .await?;

        sqlx::query("DROP TABLE IF EXISTS _transcoded_tiles")
            .execute(&mut *dst_conn)
            .await?;

        detach_db(&mut *dst_conn, "srcDb").await?;

        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&mut *dst_conn)
            .await?;

        #[expect(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "COUNT(*) is always non-negative and fits in usize for realistic tilesets"
        )]
        Ok(TranscodeStats {
            tiles_written: tiles_written as usize,
            cache_hits: 0,
            cache_encoded: tiles_written as u64,
        })
    }

    /// Flat/FlatWithHash -> Any: 3-stage pipeline with dedup cache.
    async fn run_general_path(
        self,
        src_conn: SqliteConnection,
        src_type: MbtType,
        dst: Mbtiles,
        dst_conn: SqliteConnection,
        dst_type: MbtType,
    ) -> MbtResult<TranscodeStats> {
        let (raw_tx, raw_rx) = bounded::<RawBatch>(self.channel_buffer);
        let (enc_tx, enc_rx) = bounded::<EncodedBatch>(self.channel_buffer);

        let cache = make_cache(self.cache_max_bytes);
        let stats = Arc::new(DedupStats::default());
        let transform = Arc::new(self.transform);
        let batch_size = self.batch_size;
        let max_tile_track_size = self.max_tile_track_size;

        let reader = general_reader(src_conn, src_type, raw_tx, batch_size);
        let compute = general_compute(
            raw_rx,
            enc_tx,
            transform,
            cache,
            Arc::clone(&stats),
            max_tile_track_size,
        );
        let writer = general_writer(dst, dst_conn, enc_rx, dst_type, batch_size);
        let ((), (), tiles_written) = tokio::try_join!(reader, compute, writer)?;

        Ok(TranscodeStats {
            tiles_written,
            cache_hits: stats.hits.load(Ordering::Relaxed),
            cache_encoded: stats.encoded.load(Ordering::Relaxed),
        })
    }
}

/// Construct a weighted `moka` cache bounded by `max_bytes` of encoded payload.
fn make_cache(max_bytes: u64) -> EncodedCache {
    Cache::builder()
        .max_capacity(max_bytes)
        .weigher(|_key, value: &Arc<Vec<u8>>| u32::try_from(value.len()).unwrap_or(u32::MAX))
        .build()
}

/// Parse a 32-character hex MD5 string to `u128`.
fn hex_md5_to_u128(s: &str) -> Option<u128> {
    if s.len() != 32 {
        return None;
    }
    u128::from_str_radix(s, 16).ok()
}

/// Copy all metadata rows from source to destination via ATTACH.
async fn copy_metadata(src: &Mbtiles, dst_conn: &mut SqliteConnection) -> MbtResult<()> {
    src.attach_to(dst_conn, "srcMetaDb").await?;
    sqlx::query("INSERT OR REPLACE INTO metadata SELECT name, value FROM srcMetaDb.metadata")
        .execute(&mut *dst_conn)
        .await?;
    detach_db(dst_conn, "srcMetaDb").await?;
    Ok(())
}

/// Reader: stream `tiles` rows into batches.
async fn normalized_reader(
    src_conn: &mut SqliteConnection,
    sql: &str,
    raw_tx: Sender<NormRawBatch>,
    batch_size: usize,
) -> MbtResult<()> {
    let mut stream = sqlx::query(sql).fetch(&mut *src_conn);
    let mut batch: NormRawBatch = Vec::with_capacity(batch_size);

    while let Some(row) = stream.try_next().await? {
        let tile_id: String =
            row.try_get::<String, _>("tile_id")
                .or_else(|_| -> Result<String, sqlx::Error> {
                    // DedupId schema uses integer tile_data_id
                    let id: i64 = row.try_get("tile_data_id")?;
                    Ok(id.to_string())
                })?;
        let data: Option<Vec<u8>> = row.try_get("tile_data")?;
        let Some(data) = data else { continue };

        batch.push((tile_id, data));
        if batch.len() >= batch_size {
            let full = mem::replace(&mut batch, Vec::with_capacity(batch_size));
            raw_tx
                .send_async(full)
                .await
                .map_err(|_| MbtError::TranscodeError("compute stage closed".into()))?;
        }
    }
    if !batch.is_empty() {
        raw_tx
            .send_async(batch)
            .await
            .map_err(|_| MbtError::TranscodeError("compute stage closed".into()))?;
    }
    Ok(())
}

/// Compute: transform each image on the rayon pool.
async fn normalized_compute<F>(
    raw_rx: Receiver<NormRawBatch>,
    enc_tx: Sender<NormEncBatch>,
    transform: Arc<F>,
) -> MbtResult<()>
where
    F: Fn(Vec<u8>) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>
        + Send
        + Sync
        + 'static,
{
    while let Ok(batch) = raw_rx.recv_async().await {
        let transform = Arc::clone(&transform);
        let enc_batch: NormEncBatch = spawn_blocking(move || {
            batch
                .into_par_iter()
                .filter_map(|(tile_id, data)| match (transform)(data) {
                    Ok(encoded) => Some((tile_id, encoded)),
                    Err(e) => {
                        warn!("skipping image {tile_id}: {e:#}");
                        None
                    }
                })
                .collect()
        })
        .await
        .map_err(|e| MbtError::TranscodeError(format!("join error: {e}")))?;

        if !enc_batch.is_empty() {
            enc_tx
                .send_async(enc_batch)
                .await
                .map_err(|_| MbtError::TranscodeError("writer stage closed".into()))?;
        }
    }
    Ok(())
}

/// Writer: insert encoded tiles into a temp table.
async fn normalized_writer(
    dst_conn: &mut SqliteConnection,
    enc_rx: Receiver<NormEncBatch>,
) -> MbtResult<usize> {
    sqlx::query(
        "CREATE TEMP TABLE _transcoded_tiles (tile_id TEXT PRIMARY KEY, tile_data BLOB NOT NULL)",
    )
    .execute(&mut *dst_conn)
    .await?;

    let mut total = 0usize;

    while let Ok(batch) = enc_rx.recv_async().await {
        let n = batch.len();
        let mut tx = dst_conn.begin().await?;
        for (tile_id, data) in batch {
            sqlx::query(
                "INSERT OR REPLACE INTO _transcoded_tiles (tile_id, tile_data) VALUES (?, ?)",
            )
            .bind(&tile_id)
            .bind(&data)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        total += n;
        debug!("{total} tiles encoded");
    }

    Ok(total)
}

/// Reader: stream tiles from Flat/FlatWithHash source into batches.
async fn general_reader(
    mut src_conn: SqliteConnection,
    src_type: MbtType,
    raw_tx: Sender<RawBatch>,
    batch_size: usize,
) -> MbtResult<()> {
    let sql = match src_type {
        MbtType::Flat => "SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles",
        MbtType::FlatWithHash => {
            "SELECT zoom_level, tile_column, tile_row, tile_data, tile_hash FROM tiles_with_hash"
        }
        MbtType::Normalized { .. } => {
            unreachable!("general_reader called with normalized source")
        }
    };

    let mut stream = sqlx::query(sql).fetch(&mut src_conn);
    let mut batch: RawBatch = Vec::with_capacity(batch_size);

    while let Some(row) = stream.try_next().await? {
        let z: Option<i64> = row.try_get("zoom_level")?;
        let x: Option<i64> = row.try_get("tile_column")?;
        let y: Option<i64> = row.try_get("tile_row")?;

        let Some(coord) = parse_tile_index(z, x, y) else {
            continue;
        };

        let data: Option<Vec<u8>> = row.try_get("tile_data")?;
        let Some(data) = data else { continue };

        let key = if src_type == MbtType::FlatWithHash {
            let hash: Option<String> = row.try_get("tile_hash")?;
            hash.as_deref().and_then(hex_md5_to_u128)
        } else {
            None
        };

        batch.push((coord, key, data));
        if batch.len() >= batch_size {
            let full = mem::replace(&mut batch, Vec::with_capacity(batch_size));
            raw_tx
                .send_async(full)
                .await
                .map_err(|_| MbtError::TranscodeError("compute stage closed".into()))?;
        }
    }
    if !batch.is_empty() {
        raw_tx
            .send_async(batch)
            .await
            .map_err(|_| MbtError::TranscodeError("compute stage closed".into()))?;
    }
    Ok(())
}

/// Compute: transform tiles on the rayon pool with dedup caching.
async fn general_compute<F>(
    raw_rx: Receiver<RawBatch>,
    enc_tx: Sender<EncodedBatch>,
    transform: Arc<F>,
    cache: EncodedCache,
    stats: Arc<DedupStats>,
    max_tile_track_size: usize,
) -> MbtResult<()>
where
    F: Fn(Vec<u8>) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>
        + Send
        + Sync
        + 'static,
{
    while let Ok(batch) = raw_rx.recv_async().await {
        let transform = Arc::clone(&transform);
        let cache = cache.clone();
        let stats = Arc::clone(&stats);

        let enc_batch: EncodedBatch = spawn_blocking(move || {
            batch
                .into_par_iter()
                .filter_map(|(coord, key, data)| {
                    transcode_cached(
                        coord,
                        key,
                        data,
                        transform.as_ref(),
                        &cache,
                        &stats,
                        max_tile_track_size,
                    )
                })
                .collect()
        })
        .await
        .map_err(|e| MbtError::TranscodeError(format!("join error: {e}")))?;

        if !enc_batch.is_empty() {
            enc_tx
                .send_async(enc_batch)
                .await
                .map_err(|_| MbtError::TranscodeError("writer stage closed".into()))?;
        }
    }
    Ok(())
}

/// Resolve one tile against the dedup cache, encoding only on a miss.
fn transcode_cached<F>(
    coord: TileCoord,
    key: Option<u128>,
    data: Vec<u8>,
    transform: &F,
    cache: &EncodedCache,
    stats: &DedupStats,
    max_tile_track_size: usize,
) -> Option<(TileCoord, Arc<Vec<u8>>)>
where
    F: Fn(Vec<u8>) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>,
{
    // Skip cache for large tiles — they are almost certainly unique.
    if data.len() > max_tile_track_size {
        return match (transform)(data) {
            Ok(encoded) => {
                stats.record_encode();
                Some((coord, Arc::new(encoded)))
            }
            Err(e) => {
                warn!("skipping tile {coord}: {e:#}");
                None
            }
        };
    }

    let key = key.unwrap_or_else(|| xxh3_128(&data));

    let entry = cache
        .entry(key)
        .or_try_insert_with(
            || -> Result<Arc<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
                Ok(Arc::new((transform)(data)?))
            },
        )
        .inspect_err(|e| warn!("skipping tile {coord}: {e:#}"))
        .ok()?;

    let is_fresh = entry.is_fresh();
    let arc = entry.into_value();
    if is_fresh {
        stats.record_encode();
    } else {
        stats.record_hit();
    }
    Some((coord, arc))
}

/// Writer: batch-insert encoded tiles into the destination.
async fn general_writer(
    dst: Mbtiles,
    mut dst_conn: SqliteConnection,
    enc_rx: Receiver<EncodedBatch>,
    dst_type: MbtType,
    batch_size: usize,
) -> MbtResult<usize> {
    let mut total = 0usize;
    let mut pending: Vec<(u8, u32, u32, Vec<u8>)> = Vec::with_capacity(batch_size);
    let mut last_flush = Instant::now();

    while let Ok(batch) = enc_rx.recv_async().await {
        for (coord, data) in batch {
            pending.push((coord.z, coord.x, coord.y, Arc::unwrap_or_clone(data)));
        }

        if pending.len() >= batch_size || last_flush.elapsed() >= FLUSH_INTERVAL {
            let n = pending.len();
            dst.insert_tiles(
                &mut dst_conn,
                dst_type,
                CopyDuplicateMode::Override,
                &pending,
            )
            .await?;
            pending.clear();
            total += n;
            last_flush = Instant::now();
            debug!("{total} tiles written");
        }
    }

    // Final flush.
    if !pending.is_empty() {
        let n = pending.len();
        dst.insert_tiles(
            &mut dst_conn,
            dst_type,
            CopyDuplicateMode::Override,
            &pending,
        )
        .await?;
        total += n;
    }

    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&mut dst_conn)
        .await?;

    Ok(total)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::NormalizedSchema;
use tempfile::NamedTempFile;
    use crate::metadata::temp_named_mbtiles;

    /// Helper: create a source in-memory db from SQL, run the transcoder to a
    /// temp file, and open the result for verification.
    async fn transcode_identity(
        src_script: &str,
        src_name: &str,
        dst_type: Option<MbtType>,
    ) -> (TranscodeStats, SqliteConnection, tempfile::TempDir) {
        let (_mbt, _conn, src_file) = temp_named_mbtiles(src_name, src_script).await;

                    let dst_file = NamedTempFile::with_suffix("mbtiles").unwrap();

        let mut builder = MbtilesTranscoder::new(src_file, dst_file.clone(), |data| Ok(data));
        if let Some(dt) = dst_type {
            builder = builder.dst_type(dt);
        }
        let stats = builder.run().await.unwrap();

        let dst_mbt = Mbtiles::new(&dst_file).unwrap();
        let conn = dst_mbt.open_readonly().await.unwrap();
        (stats, conn, dir)
    }

    /// Helper for tests needing a real source file (e.g. DedupId with
    /// `WITHOUT ROWID` tables that conflict with shared-cache locking).
    async fn transcode_identity_file(
        src_script: &str,
        dst_type: Option<MbtType>,
    ) -> (TranscodeStats, SqliteConnection, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let src_file = dir.path().join("source.mbtiles");
        let dst_file = dir.path().join("output.mbtiles");

        let src_mbt = Mbtiles::new(&src_file).unwrap();
        let mut src_conn = src_mbt.open_or_new().await.unwrap();
        sqlx::raw_sql(src_script)
            .execute(&mut src_conn)
            .await
            .unwrap();
        drop(src_conn);

        let mut builder = MbtilesTranscoder::new(src_file, dst_file.clone(), |data| Ok(data));
        if let Some(dt) = dst_type {
            builder = builder.dst_type(dt);
        }
        let stats = builder.run().await.unwrap();

        let dst_mbt = Mbtiles::new(&dst_file).unwrap();
        let conn = dst_mbt.open_readonly().await.unwrap();
        (stats, conn, dir)
    }

    #[actix_rt::test]
    async fn transcode_flat_to_flat() {
        let script = include_str!("../../tests/fixtures/mbtiles/world_cities.sql");
        let (stats, mut conn, _dir) =
            transcode_identity(script, "tc_flat_flat", Some(MbtType::Flat)).await;

        assert!(stats.tiles_written, 8);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tiles")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(count, 8);
    }

    #[actix_rt::test]
    async fn transcode_flat_to_flat_with_hash() {
        let script = include_str!("../../tests/fixtures/mbtiles/world_cities.sql");
        let (stats, mut conn, _dir) =
            transcode_identity(script, "tc_flat_fwh", Some(MbtType::FlatWithHash)).await;

        assert_eq!(stats.tiles_written, 8);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tiles_with_hash")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(count, 8);
    }

    #[actix_rt::test]
    async fn transcode_normalized_to_normalized() {
        let script = include_str!("../../tests/fixtures/mbtiles/geography-class-png.sql");
        let (stats, mut conn, _dir) = transcode_identity(
            script,
            "tc_norm_norm",
            Some(MbtType::Normalized {
                hash_view: true,
                schema: NormalizedSchema::Hash,
            }),
        )
        .await;

        assert_eq!(stats.tiles_written, 5);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM map")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(count, 5);
    }

    #[actix_rt::test]
    async fn transcode_normalized_to_flat() {
        let script = include_str!("../../tests/fixtures/mbtiles/geography-class-png.sql");
        let (stats, mut conn, _dir) =
            transcode_identity(script, "tc_norm_flat", Some(MbtType::Flat)).await;

        assert_eq!(stats.tiles_written, 5);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tiles")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(count, 5);
    }

    #[actix_rt::test]
    async fn transcode_dedup_id_to_hash_normalized() {
        let script = include_str!("../../tests/fixtures/mbtiles/normalized-dedup-id.sql");
        let (stats, mut conn, _dir) = transcode_identity_file(
            script,
            Some(MbtType::Normalized {
                hash_view: true,
                schema: NormalizedSchema::Hash,
            }),
        )
        .await;

        assert_eq!(stats.tiles_written, 5);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM map")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(count, 5);
    }

    #[actix_rt::test]
    async fn transcode_dedup_id_to_flat() {
        let script = include_str!("../../tests/fixtures/mbtiles/normalized-dedup-id.sql");
        let (stats, mut conn, _dir) = transcode_identity_file(script, Some(MbtType::Flat)).await;

        assert!(stats.tiles_written > 0);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tiles")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(count, 5);
    }

    #[actix_rt::test]
    async fn transcode_normalized_no_redundant_transforms() {
        // 2 unique images, each > 1KB (to exceed max_tile_track_size),
        // referenced by 5 map entries. The transform must be called
        // exactly 2 times — once per unique image.
        let tile_a: String = format!("X'{}'", "AA".repeat(2048));
        let tile_b: String = format!("X'{}'", "BB".repeat(2048));
        let script = format!(
            "CREATE TABLE map (zoom_level INTEGER, tile_column INTEGER, \
                              tile_row INTEGER, tile_id TEXT);\
             INSERT INTO map VALUES(0,0,0,'aaa');\
             INSERT INTO map VALUES(1,0,0,'aaa');\
             INSERT INTO map VALUES(1,0,1,'bbb');\
             INSERT INTO map VALUES(1,1,0,'bbb');\
             INSERT INTO map VALUES(1,1,1,'aaa');\
             CREATE TABLE images (tile_data BLOB, tile_id TEXT);\
             INSERT INTO images VALUES({tile_a},'aaa');\
             INSERT INTO images VALUES({tile_b},'bbb');\
             CREATE TABLE metadata (name TEXT, value TEXT);\
             CREATE UNIQUE INDEX map_index ON map (zoom_level, tile_column, tile_row);\
             CREATE UNIQUE INDEX images_id ON images (tile_id);\
             INSERT INTO metadata VALUES('name','test');\
             INSERT INTO metadata VALUES('format','pbf');"
        );

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_clone = Arc::clone(&call_count);

        let (_mbt, _conn, src_file) = temp_named_mbtiles("tc_dedup", script).await;
        let dst_file = NamedTempFile::with_suffix("mbtiles").unwrap();

        let src_mbt = Mbtiles::new(src_file.path()).unwrap();
        let mut src_conn = src_mbt.open_or_new().await.unwrap();
        sqlx::raw_sql(&script)
            .execute(&mut src_conn)
            .await
            .unwrap();
        drop(src_conn);

        let stats = MbtilesTranscoder::new(src_file.path(), dst_file.path(), move |data| {
            call_count_clone.fetch_add(1, Ordering::Relaxed);
            Ok(data)
        })
        .dst_type(MbtType::Normalized {
            hash_view: true,
            schema: NormalizedSchema::Hash,
        })
        .run()
        .await
        .unwrap();

        insta::assert_snapshot!(stats, @"tiles_written=5, cache_hits=0, cache_encoded=2");
        let calls = call_count.load(Ordering::Relaxed);
        assert_eq!(calls, 2, "transform must be called once per unique image, not per map entry");
    }

    #[actix_rt::test]
    async fn transcode_dedup_cache_avoids_redundant_transforms() {
        let script = ;
        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_clone = Arc::clone(&call_count);

        let (_mbt, _conn, src_file) = temp_named_mbtiles("tc_dedup", include_str!("../../tests/fixtures/mbtiles/world_cities.sql")).await;
        let dst_file = NamedTempFile::with_suffix("mbtiles").unwrap();

        let stats = MbtilesTranscoder::new(src_file, dst_file.path(), move |data| {
            call_count_clone.fetch_add(1, Ordering::Relaxed);
            Ok(data)
        })
        .dst_type(MbtType::Flat)
        .run()
        .await
        .unwrap();

        insta::assert_snapshot!(stats, @"tiles_written=8, cache_hits=4, cache_encoded=4");
        let calls = call_count.load(Ordering::Relaxed);
        assert_eq!(calls as u64, stats.cache_encoded);
    }
}

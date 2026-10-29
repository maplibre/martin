use std::path::Path;
use std::sync::mpsc::sync_channel;
use std::thread;

use mbtiles::sqlx::{AssertSqlSafe, SqliteConnection, raw_sql};
use mbtiles::{
    AGG_TILES_HASH, AggTilesHasher, CopyDuplicateMode, MbtType, Mbtiles, NormalizedSchema, TileDedup,
    init_mbtiles_schema, is_empty_database,
};
use tilejson::TileJSON;
use tokio::runtime::Handle;

use super::{EncodedTile, TileSink};
use crate::{TileGenError, TileGenResult, TileOrder};

/// Written batches waiting for the hashing thread.
const HASH_QUEUE: usize = 2;

/// How [`MbtilesSink::create_with`] lays out a new file and finishes its metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MbtilesOptions {
    /// The schema of the new file.
    pub mbt_type: MbtType,
    /// Whether [`TileSink::finish`] writes `agg_tiles_hash`, unless the metadata edits set or delete it.
    pub agg_tiles_hash: bool,
    /// Metadata values set after the generator's own.
    pub set_meta: Vec<(String, String)>,
    /// Metadata keys deleted after the generator wrote its own, which they may name.
    pub delete_meta: Vec<String>,
}

impl Default for MbtilesOptions {
    /// A deduplicated (`DedupId`) file with `agg_tiles_hash` and unedited metadata.
    fn default() -> Self {
        Self {
            mbt_type: MbtType::Normalized {
                hash_view: false,
                schema: NormalizedSchema::DedupId,
            },
            agg_tiles_hash: true,
            set_meta: Vec::new(),
            delete_meta: Vec::new(),
        }
    }
}

impl MbtilesOptions {
    /// Whether `finish` computes `agg_tiles_hash`: requested, and not set or deleted by the edits.
    fn hashes(&self) -> bool {
        self.agg_tiles_hash
            && !self.delete_meta.iter().any(|key| key == AGG_TILES_HASH)
            && !self.set_meta.iter().any(|(key, _)| key == AGG_TILES_HASH)
    }
}

/// Writes a new `MBTiles` file through [`Mbtiles::bulk_write`], hashing the written tiles on
/// another thread for `agg_tiles_hash`.
/// The mbtiles crate is async, so the sink blocks on the tokio runtime it was created in.
pub struct MbtilesSink {
    runtime: Handle,
    mbt: Mbtiles,
    conn: SqliteConnection,
    hasher: Option<AggTilesHasher>,
    options: MbtilesOptions,
}

impl MbtilesSink {
    /// A deduplicated (`DedupId`) file with `agg_tiles_hash`, as [`MbtilesOptions::default`].
    ///
    /// Must be called on a blocking thread of a tokio runtime, e.g. inside [`tokio::task::spawn_blocking`].
    pub fn create(path: &Path) -> TileGenResult<Self> {
        Self::create_with(path, MbtilesOptions::default())
    }

    /// Must be called on a blocking thread of a tokio runtime, e.g. inside [`tokio::task::spawn_blocking`].
    pub fn create_with(path: &Path, options: MbtilesOptions) -> TileGenResult<Self> {
        let runtime = Handle::current();
        let mbt = Mbtiles::new(path)?;
        let (conn, algorithm) = runtime.block_on(async {
            let mut conn = mbt.open_or_new().await?;
            if !is_empty_database(&mut conn).await? {
                return Err(TileGenError::OutputNotEmpty(path.to_path_buf()));
            }
            init_mbtiles_schema(&mut conn, options.mbt_type, false).await?;
            raw_sql(AssertSqlSafe("PRAGMA page_size = 4096; VACUUM;"))
                .execute(&mut conn)
                .await
                .map_err(mbtiles::MbtError::from)?;
            let algorithm = mbt.get_hash_algorithm(&mut conn).await?;
            Ok((conn, algorithm))
        })?;
        Ok(Self {
            runtime,
            mbt,
            conn,
            hasher: options.hashes().then(|| AggTilesHasher::new(algorithm)),
            options,
        })
    }
}

impl TileSink for MbtilesSink {
    fn tile_order(&self) -> TileOrder {
        TileOrder::Tms
    }

    fn write_all(&mut self, batches: &mut dyn Iterator<Item = TileGenResult<Vec<EncodedTile>>>) -> TileGenResult<()> {
        let Self {
            runtime,
            mbt,
            conn,
            hasher,
            options,
        } = self;
        let mut running = hasher.take();
        let hashing_in = running.is_some();
        let (tx, rx) = sync_channel::<Vec<EncodedTile>>(HASH_QUEUE);
        thread::scope(|scope| {
            let hashing = scope.spawn(move || {
                for batch in rx {
                    if let Some(hasher) = &mut running {
                        for tile in &batch {
                            hasher.push(tile.coord, &tile.data);
                        }
                    }
                }
                running
            });
            let write = mbt.bulk_write(conn, options.mbt_type, CopyDuplicateMode::Abort, |writer| {
                for batch in batches {
                    let batch = batch?;
                    for tile in &batch {
                        let dedup = tile.dedup_key.map_or(TileDedup::Unique, TileDedup::Key);
                        writer.write(tile.coord, &tile.data, dedup)?;
                    }
                    tx.send(batch).ok();
                }
                Ok(())
            });
            let written = runtime.block_on(write);
            drop(tx);
            let hashed = hashing.join().ok().flatten();
            debug_assert!(
                written.is_err() || !hashing_in || hashed.as_ref().is_some_and(AggTilesHasher::in_key_order),
                "tiles came out of TMS order"
            );
            *hasher = if written.is_ok() { hashed } else { None };
            written
        })
    }

    fn finish(mut self, metadata: &TileJSON) -> TileGenResult<()> {
        self.runtime.block_on(async {
            self.mbt.insert_metadata(&mut self.conn, metadata).await?;
            // Before the hash, so that it uses an edited `hash_algorithm`.
            self.mbt
                .edit_metadata(&mut self.conn, &self.options.set_meta, &self.options.delete_meta)
                .await?;
            if self.options.hashes() {
                match self.hasher {
                    Some(hasher) => self.mbt.finish_agg_tiles_hash(&mut self.conn, hasher).await?,
                    None => self.mbt.update_agg_tiles_hash(&mut self.conn).await?,
                };
            }
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
                    Ok(vec![tile(0, 0, 0, b"root", None), tile(1, 0, 1, b"fill", Some(1))]),
                    Ok(vec![tile(1, 0, 0, b"fill", Some(1)), tile(1, 1, 1, b"leaf", None)]),
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
        assert_eq!(mbt.get_tile(&mut conn, 1, 0, 1).await.unwrap().unwrap(), b"fill");
        assert_eq!(mbt.get_tile(&mut conn, 1, 1, 1).await.unwrap().unwrap(), b"leaf");
        let blobs: i64 = mbtiles::sqlx::query_scalar("SELECT count(*) FROM tiles_data")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(blobs, 3);
    }

    #[tokio::test]
    async fn writes_the_requested_schema_without_agg_tiles_hash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.mbtiles");
        tokio::task::spawn_blocking({
            let path = path.clone();
            move || {
                let options = MbtilesOptions {
                    mbt_type: MbtType::Flat,
                    agg_tiles_hash: false,
                    ..MbtilesOptions::default()
                };
                let mut sink = MbtilesSink::create_with(&path, options).unwrap();
                let batches = vec![Ok(vec![
                    tile(1, 0, 1, b"fill", Some(1)),
                    tile(1, 0, 0, b"fill", Some(1)),
                ])];
                sink.write_all(&mut batches.into_iter()).unwrap();
                sink.finish(&tilejson::tilejson! { tiles: vec![] }).unwrap();
            }
        })
        .await
        .unwrap();

        let mbt = Mbtiles::new(&path).unwrap();
        let mut conn = mbt.open_readonly().await.unwrap();
        assert_eq!(mbt.detect_type(&mut conn).await.unwrap(), MbtType::Flat);
        assert_eq!(mbt.get_agg_tiles_hash(&mut conn).await.unwrap(), None);
        assert_eq!(mbt.get_tile(&mut conn, 1, 0, 0).await.unwrap().unwrap(), b"fill");
    }

    #[tokio::test]
    async fn edits_metadata_before_hashing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.mbtiles");
        tokio::task::spawn_blocking({
            let path = path.clone();
            move || {
                let options = MbtilesOptions {
                    set_meta: vec![
                        ("name".to_owned(), "edited".to_owned()),
                        ("hash_algorithm".to_owned(), "xxh3".to_owned()),
                    ],
                    delete_meta: vec!["format".to_owned()],
                    ..MbtilesOptions::default()
                };
                let mut sink = MbtilesSink::create_with(&path, options).unwrap();
                let batches = vec![Ok(vec![tile(0, 0, 0, b"root", None)])];
                sink.write_all(&mut batches.into_iter()).unwrap();
                let mut metadata = tilejson::tilejson! { tiles: vec![] };
                metadata.name = Some("generated".to_owned());
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
            mbt.get_metadata_value(&mut conn, "name").await.unwrap().as_deref(),
            Some("edited")
        );
        assert_eq!(mbt.get_metadata_value(&mut conn, "format").await.unwrap(), None);
        assert_eq!(
            mbt.get_metadata_value(&mut conn, "hash_algorithm")
                .await
                .unwrap()
                .as_deref(),
            Some("xxh3")
        );
    }

    #[tokio::test]
    async fn keeps_an_agg_tiles_hash_the_edits_set() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.mbtiles");
        tokio::task::spawn_blocking({
            let path = path.clone();
            move || {
                let options = MbtilesOptions {
                    set_meta: vec![("agg_tiles_hash".to_owned(), "given".to_owned())],
                    ..MbtilesOptions::default()
                };
                let mut sink = MbtilesSink::create_with(&path, options).unwrap();
                let batches = vec![Ok(vec![tile(0, 0, 0, b"root", None)])];
                sink.write_all(&mut batches.into_iter()).unwrap();
                sink.finish(&tilejson::tilejson! { tiles: vec![] }).unwrap();
            }
        })
        .await
        .unwrap();

        let mbt = Mbtiles::new(&path).unwrap();
        let mut conn = mbt.open_readonly().await.unwrap();
        assert_eq!(
            mbt.get_agg_tiles_hash(&mut conn).await.unwrap().as_deref(),
            Some("given")
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "tiles came out of TMS order")]
    fn tiles_out_of_tms_order_panic_in_debug() {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let _runtime = runtime.enter();
        let dir = tempfile::tempdir().unwrap();
        let mut sink = MbtilesSink::create(&dir.path().join("out.mbtiles")).unwrap();
        let batches = vec![Ok(vec![tile(1, 0, 0, b"a", None), tile(1, 0, 1, b"b", None)])];
        sink.write_all(&mut batches.into_iter()).unwrap();
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

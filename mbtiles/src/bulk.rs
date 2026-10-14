//! Synchronous bulk tile writing. `SQLite` itself is synchronous: sqlx's async driver sends every
//! statement to a per-connection worker thread, so writing millions of tiles is much faster on the
//! connection's raw rusqlite handle.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::num::NonZeroUsize;

use sqlite_hashes::rusqlite::types::ToSql;
use sqlite_hashes::rusqlite::{Connection, OptionalExtension as _, Statement, params};
use sqlx::SqliteConnection;
use xxhash_rust::xxh3::xxh3_64;

use crate::{
    CopyDuplicateMode, HASH_ALGORITHM, HashAlgorithm, MbtError, MbtResult, MbtType, Mbtiles,
    NormalizedSchema, TileCoord, action_with_rusqlite, invert_y_value,
};

/// Large enough to amortize commits, small enough to bound the journal of an interrupted write.
const DEFAULT_BATCH_SIZE: NonZeroUsize = NonZeroUsize::new(1 << 16).expect("non-zero");

/// What the caller knows about a tile's bytes, so that deduplicating schemas store repeated bytes
/// once without hashing what they need not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileDedup {
    /// Nothing: the bytes are compared by their hash, as [`Mbtiles::insert_tiles`] does.
    Unknown,
    /// No other tile of this write has these bytes.
    Unique,
    /// Tiles with the same key have identical bytes. A key reused for other bytes fails the write
    /// with [`MbtError::DedupKeyMismatch`]. Keys are scoped to one [`Mbtiles::bulk_write`].
    Key(u64),
}

impl Mbtiles {
    /// Writes tiles in bulk: `write` gets a [`MbtilesBulkWriter`], and the tiles are committed when it
    /// returns `Ok` and rolled back when it fails or panics. The schema must already exist.
    ///
    /// Nothing else can use `conn` until `write` returns, and `write` never sees the raw connection,
    /// so the writer has it to itself. It holds the write lock from the start, so other connections
    /// to the file wait. By default it commits every 65,536 tiles to bound the journal, so a failed
    /// write keeps the tiles of those earlier commits; set [`MbtilesBulkWriter::batch_size`] to `None`
    /// for a write that is all or nothing. Into a file that holds no tiles yet, it writes with
    /// `synchronous` off, as such a file is regenerated rather than recovered after an OS crash.
    ///
    /// Tiles are hashed with the file's own [`HashAlgorithm`]; the `agg_tiles_hash` metadata is left to
    /// the caller ([`Mbtiles::update_agg_tiles_hash`]). `write` runs synchronously on the calling
    /// thread: call this from a blocking context, e.g. [`tokio::task::spawn_blocking`] or a
    /// dedicated thread, rather than from an async worker thread.
    pub async fn bulk_write<R, E: From<MbtError>>(
        &self,
        conn: &mut SqliteConnection,
        mbt_type: MbtType,
        on_duplicate: CopyDuplicateMode,
        write: impl FnOnce(&mut MbtilesBulkWriter<'_>) -> Result<R, E>,
    ) -> Result<R, E> {
        action_with_rusqlite(conn, |conn| {
            let mut writer = MbtilesBulkWriter::new(conn, self.filepath(), mbt_type, on_duplicate)?;
            let result = write(&mut writer);
            if result.is_ok() {
                writer.commit()?;
            }
            Ok(result)
        })
        .await
        .map_err(E::from)
        .and_then(|result| result)
    }
}

/// Inserts tiles with prepared statements reused across tiles, in large transactions. Only
/// [`Mbtiles::bulk_write`] creates one, and it owns it.
pub struct MbtilesBulkWriter<'c> {
    /// Tiles per transaction, 65,536 by default; it may change at any point of the write. `None` writes
    /// everything in one transaction, which a failure rolls back entirely, but whose journal (or WAL)
    /// grows with the data until it commits.
    pub batch_size: Option<NonZeroUsize>,
    conn: &'c Connection,
    target: Target<'c>,
    algorithm: HashAlgorithm,
    pending: usize,
    /// The connection's own setting, restored when the writer is done.
    synchronous: i64,
    done: bool,
}

enum Target<'c> {
    /// The statements [`Mbtiles::insert_tiles`] uses: tiles keyed by their hash, if `hashed`, and the
    /// blob statements of the hash-normalized schema.
    Shared {
        tiles: Statement<'c>,
        blobs: Vec<Statement<'c>>,
        hashed: bool,
        /// Hashes of keyed tiles (`None` if not `hashed`), so a repeated tile is hashed and its blob
        /// stored once.
        known: Keyed<Option<String>>,
    },
    /// Blob ids come from the caller's keys where it has them, so most tiles are never hashed.
    DedupId {
        shallow: Statement<'c>,
        blobs: Blobs<'c>,
        ids: Keyed<i64>,
    },
}

/// The dedup-id blobs, with ids past those in use. While `temp.tile_ids` is in use on the connection,
/// every new blob is looked up and recorded there, so the index stays complete for later
/// [`TileDedup::Unknown`] tiles and [`Mbtiles::insert_tiles`]; until then, hinted tiles are not hashed.
struct Blobs<'c> {
    insert: Statement<'c>,
    next_id: i64,
    /// Lookup and insert into `temp.tile_ids`.
    index: Option<(Statement<'c>, Statement<'c>)>,
}

impl<'c> MbtilesBulkWriter<'c> {
    fn new(
        conn: &'c Connection,
        filepath: &str,
        mbt_type: MbtType,
        on_duplicate: CopyDuplicateMode,
    ) -> MbtResult<Self> {
        let synchronous = conn.pragma_query_value(None, "synchronous", |row| row.get(0))?;
        // A file written from empty is regenerated rather than recovered, so syncing its commits buys
        // nothing. Tiles already there keep the connection's durability: without syncing, an OS crash
        // during the write could corrupt them.
        let sql = "SELECT EXISTS (SELECT 1 FROM tiles)";
        let has_tiles: bool = conn.query_row(sql, [], |row| row.get(0))?;
        if !has_tiles {
            conn.pragma_update(None, "synchronous", "OFF")?;
        }
        // The write lock comes first, so that what is read below holds for the whole write.
        if let Err(err) = conn.execute_batch("BEGIN IMMEDIATE") {
            let _ = conn.pragma_update(None, "synchronous", synchronous);
            return Err(err.into());
        }
        let prepared = hash_algorithm(conn, filepath).and_then(|algorithm| {
            Ok((
                algorithm,
                Target::new(conn, mbt_type, on_duplicate, algorithm)?,
            ))
        });
        match prepared {
            Ok((algorithm, target)) => Ok(Self {
                batch_size: Some(DEFAULT_BATCH_SIZE),
                conn,
                target,
                algorithm,
                pending: 0,
                synchronous,
                done: false,
            }),
            Err(err) => {
                // The caller already has the error that matters.
                let _ = end(conn, "ROLLBACK", synchronous);
                Err(err)
            }
        }
    }

    /// `coord` uses XYZ rows.
    pub fn write(&mut self, coord: TileCoord, data: &[u8], dedup: TileDedup) -> MbtResult<()> {
        let (z, x, y) = (coord.z(), coord.x(), invert_y_value(coord.z(), coord.y()));
        let algorithm = self.algorithm;
        match &mut self.target {
            Target::Shared {
                tiles,
                blobs,
                hashed,
                known,
            } => {
                let hash = || hashed.then(|| algorithm.hash(data));
                let (hash, new_blob) = match dedup {
                    // Checked in every schema, also those that store no hash.
                    TileDedup::Key(key) => known.get_or_insert(key, data, || Ok(hash()))?,
                    TileDedup::Unique | TileDedup::Unknown => (hash(), true),
                };
                // The shared statements number their parameters so that each binds a prefix of these.
                if new_blob {
                    for blobs in blobs {
                        let values: [&dyn ToSql; 2] = [&data, &hash];
                        blobs.execute(&values[..blobs.parameter_count()])?;
                    }
                }
                let values: [&dyn ToSql; 5] = [&z, &x, &y, &data, &hash];
                tiles.execute(&values[..tiles.parameter_count()])?;
            }
            Target::DedupId {
                shallow,
                blobs,
                ids,
            } => {
                let conn = self.conn;
                let id = match dedup {
                    TileDedup::Unique => blobs.id(conn, algorithm, data, false)?,
                    TileDedup::Key(key) => {
                        let new = || blobs.id(conn, algorithm, data, false);
                        ids.get_or_insert(key, data, new)?.0
                    }
                    TileDedup::Unknown => blobs.id(conn, algorithm, data, true)?,
                };
                shallow.execute(params![z, x, y, id])?;
            }
        }
        self.pending += 1;
        if self
            .batch_size
            .is_some_and(|size| self.pending >= size.get())
        {
            self.checkpoint()?;
        }
        Ok(())
    }

    /// Commits the tiles written so far and goes on in a new transaction, e.g. to bound by time how
    /// much an interrupted write loses, as [`batch_size`](Self::batch_size) bounds it by tile count.
    pub fn checkpoint(&mut self) -> MbtResult<()> {
        self.conn.execute_batch("COMMIT; BEGIN IMMEDIATE")?;
        self.pending = 0;
        // Another connection may have written between the two.
        if let Target::DedupId { blobs, .. } = &mut self.target {
            blobs.next_id = blobs.next_id.max(next_blob_id(self.conn)?);
        }
        Ok(())
    }

    fn commit(mut self) -> MbtResult<()> {
        // On failure, dropping rolls back what is left and retries restoring `synchronous`.
        end(self.conn, "COMMIT", self.synchronous)?;
        self.done = true;
        Ok(())
    }
}

impl Drop for MbtilesBulkWriter<'_> {
    fn drop(&mut self) {
        if !self.done {
            // Nothing to report from a drop; the caller already has the error that caused it.
            let _ = end(self.conn, "ROLLBACK", self.synchronous);
        }
    }
}

impl<'c> Target<'c> {
    fn new(
        conn: &'c Connection,
        mbt_type: MbtType,
        on_duplicate: CopyDuplicateMode,
        algorithm: HashAlgorithm,
    ) -> MbtResult<Self> {
        Ok(
            if mbt_type.normalized_schema() == Some(NormalizedSchema::DedupId) {
                Self::DedupId {
                    shallow: conn.prepare(&format!(
                    "INSERT {} INTO tiles_shallow (zoom_level, tile_column, tile_row, tile_data_id)
                     VALUES (?1, ?2, ?3, ?4)",
                    on_duplicate.to_sql()
                ))?,
                    blobs: Blobs::new(conn, algorithm)?,
                    ids: Keyed::default(),
                }
            } else {
                let (tiles, blobs) = Mbtiles::get_insert_sql(mbt_type, on_duplicate);
                Self::Shared {
                    tiles: conn.prepare(&tiles)?,
                    blobs: blobs
                        .iter()
                        .map(|sql| conn.prepare(sql))
                        .collect::<Result<_, _>>()?,
                    hashed: matches!(mbt_type, MbtType::FlatWithHash | MbtType::Normalized { .. }),
                    known: Keyed::default(),
                }
            },
        )
    }
}

/// What the writer remembers per [`TileDedup::Key`], with the length and `xxh3` of the key's first
/// bytes: a few nanoseconds per tile, against a statement's microsecond, to catch a key reused for
/// other bytes, which would otherwise store a wrong tile.
struct Keyed<V>(HashMap<u64, (V, usize, u64)>);

impl<V> Default for Keyed<V> {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl<V: Clone> Keyed<V> {
    /// The key's value, and whether `make` just created it.
    fn get_or_insert(
        &mut self,
        key: u64,
        data: &[u8],
        make: impl FnOnce() -> MbtResult<V>,
    ) -> MbtResult<(V, bool)> {
        let fingerprint = (data.len(), xxh3_64(data));
        match self.0.entry(key) {
            Entry::Occupied(known) => {
                let (value, len, hash) = known.get();
                if (*len, *hash) != fingerprint {
                    return Err(MbtError::DedupKeyMismatch(key));
                }
                Ok((value.clone(), false))
            }
            Entry::Vacant(slot) => {
                let value = make()?;
                slot.insert((value.clone(), fingerprint.0, fingerprint.1));
                Ok((value, true))
            }
        }
    }
}

/// Ends the transaction and restores `synchronous`, also when there is no transaction left to end.
fn end(conn: &Connection, sql: &str, synchronous: i64) -> MbtResult<()> {
    let ended = conn.execute_batch(sql);
    conn.pragma_update(None, "synchronous", synchronous)?;
    Ok(ended?)
}

impl<'c> Blobs<'c> {
    fn new(conn: &'c Connection, algorithm: HashAlgorithm) -> MbtResult<Self> {
        let mut blobs = Self {
            insert: conn
                .prepare("INSERT INTO tiles_data (tile_data_id, tile_data) VALUES (?1, ?2)")?,
            next_id: next_blob_id(conn)?,
            index: None,
        };
        // An earlier `insert_tiles` on this connection left the index in use.
        let sql = "SELECT EXISTS (SELECT 1 FROM temp.sqlite_master WHERE name = 'tile_ids')";
        let indexed: bool = conn.query_row(sql, [], |row| row.get(0))?;
        if indexed {
            blobs.index = Some(tile_ids(conn, algorithm, &mut blobs.next_id)?);
        }
        Ok(blobs)
    }

    /// The id of a blob holding `data`, through the index while it is in use; `by_hash` starts using it.
    fn id(
        &mut self,
        conn: &'c Connection,
        algorithm: HashAlgorithm,
        data: &[u8],
        by_hash: bool,
    ) -> MbtResult<i64> {
        if by_hash && self.index.is_none() {
            // Indexes every blob stored so far, this write's included.
            self.index = Some(tile_ids(conn, algorithm, &mut self.next_id)?);
        }
        let Some((find, remember)) = &mut self.index else {
            return new_blob(&mut self.insert, &mut self.next_id, data);
        };
        let hash = algorithm.hash(data);
        if let Some(id) = find.query_row([&hash], |row| row.get(0)).optional()? {
            return Ok(id);
        }
        let id = new_blob(&mut self.insert, &mut self.next_id, data)?;
        remember.execute(params![id, hash])?;
        Ok(id)
    }
}

fn new_blob(insert: &mut Statement<'_>, next_id: &mut i64, data: &[u8]) -> MbtResult<i64> {
    let id = *next_id;
    insert.execute(params![id, data])?;
    *next_id += 1;
    Ok(id)
}

fn next_blob_id(conn: &Connection) -> MbtResult<i64> {
    let sql = "SELECT coalesce(max(tile_data_id), 0) + 1 FROM tiles_data";
    Ok(conn.query_row(sql, [], |row| row.get(0))?)
}

/// The file's [`HASH_ALGORITHM`], so that its hashes stay verifiable.
fn hash_algorithm(conn: &Connection, filepath: &str) -> MbtResult<HashAlgorithm> {
    let sql = "SELECT value FROM metadata WHERE name = ?1";
    let value: Option<String> = conn
        .query_row(sql, [HASH_ALGORITHM], |row| row.get(0))
        .optional()?;
    value.map_or(Ok(HashAlgorithm::Md5), |algorithm| {
        HashAlgorithm::parse(&algorithm).ok_or_else(|| MbtError::UnsupportedHashAlgorithm {
            algorithm,
            filepath: filepath.into(),
        })
    })
}

/// The `temp.tile_ids` hash index [`Mbtiles::insert_tiles`] keeps for the connection, shared so that
/// either finds the blobs the other stored. New ids continue past the ids it already holds.
fn tile_ids<'c>(
    conn: &'c Connection,
    algorithm: HashAlgorithm,
    next_id: &mut i64,
) -> MbtResult<(Statement<'c>, Statement<'c>)> {
    conn.execute_batch(&NormalizedSchema::create_tile_ids_sql(algorithm))?;
    let sql = "SELECT coalesce(max(tile_data_id), 0) FROM temp.tile_ids";
    let max: i64 = conn.query_row(sql, [], |row| row.get(0))?;
    *next_id = (*next_id).max(max + 1);
    Ok((
        conn.prepare("SELECT tile_data_id FROM temp.tile_ids WHERE tile_hash = ?1")?,
        conn.prepare("INSERT INTO temp.tile_ids (tile_data_id, tile_hash) VALUES (?1, ?2)")?,
    ))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use sqlx::{SqliteConnection, query, query_scalar};

    use super::*;
    use crate::{
        AggHashType, IntegrityCheckType, action_with_rusqlite, anonymous_mbtiles,
        init_mbtiles_schema,
    };

    type Row = (i64, i64, i64, Vec<u8>);
    /// XYZ coordinate, bytes, dedup hint.
    type TestTile = (u8, u32, u32, Vec<u8>, TileDedup);

    const DEDUP_ID: MbtType = MbtType::Normalized {
        hash_view: false,
        schema: NormalizedSchema::DedupId,
    };

    /// Tiles with repeated bytes, as a generator would write them: a keyed fill, unique tiles, and a
    /// repeated tile the caller knows nothing about.
    fn tiles() -> Vec<TestTile> {
        let mut tiles = Vec::new();
        for z in 0..4 {
            for x in 0..1 << z {
                for y in 0..1 << z {
                    tiles.push(match (x + y) % 3 {
                        0 => (z, x, y, b"fill".to_vec(), TileDedup::Key(7)),
                        1 => (z, x, y, b"same".to_vec(), TileDedup::Unknown),
                        _ => (
                            z,
                            x,
                            y,
                            format!("{z}/{x}/{y}").into_bytes(),
                            TileDedup::Unique,
                        ),
                    });
                }
            }
        }
        tiles
    }

    async fn schema(mbt_type: MbtType) -> (Mbtiles, SqliteConnection) {
        let (mbt, mut conn) = anonymous_mbtiles("").await;
        init_mbtiles_schema(&mut conn, mbt_type, false)
            .await
            .unwrap();
        (mbt, conn)
    }

    async fn bulk_write(
        mbt: &Mbtiles,
        conn: &mut SqliteConnection,
        mbt_type: MbtType,
        on_duplicate: CopyDuplicateMode,
        tiles: &[TestTile],
    ) -> MbtResult<()> {
        mbt.bulk_write(conn, mbt_type, on_duplicate, |writer| {
            for (z, x, y, data, dedup) in tiles {
                writer.write(TileCoord::new_unchecked(*z, *x, *y), data, *dedup)?;
            }
            Ok(())
        })
        .await
    }

    async fn rows(conn: &mut SqliteConnection) -> Vec<Row> {
        let sql = query!(
            r#"SELECT zoom_level AS "z!", tile_column AS "x!", tile_row AS "y!", tile_data AS "data!"
               FROM tiles ORDER BY 1, 2, 3"#
        );
        let rows = sql.fetch_all(conn).await.unwrap();
        rows.into_iter().map(|r| (r.z, r.x, r.y, r.data)).collect()
    }

    async fn tile_count(conn: &mut SqliteConnection) -> usize {
        let sql = query_scalar!("SELECT count(*) FROM tiles");
        usize::try_from(sql.fetch_one(conn).await.unwrap()).unwrap()
    }

    /// Blobs of the deduplicating schemas.
    async fn blob_count(conn: &mut SqliteConnection, mbt_type: MbtType) -> usize {
        let n = match mbt_type.normalized_schema() {
            Some(NormalizedSchema::Hash) => {
                let sql = query_scalar!("SELECT count(*) FROM images");
                sql.fetch_one(conn).await
            }
            Some(NormalizedSchema::DedupId) => {
                let sql = query_scalar!("SELECT count(*) FROM tiles_data");
                sql.fetch_one(conn).await
            }
            None => panic!("{mbt_type} stores no separate blobs"),
        };
        usize::try_from(n.unwrap()).unwrap()
    }

    /// In `Abort` mode, which also checks that repeated bytes are no duplicate tiles.
    #[rstest]
    #[case::flat(MbtType::Flat)]
    #[case::flat_with_hash(MbtType::FlatWithHash)]
    #[case::hash_normalized(MbtType::Normalized { hash_view: false, schema: NormalizedSchema::Hash })]
    #[case::dedup_id(DEDUP_ID)]
    #[case::cache(MbtType::Cache)]
    #[actix_rt::test]
    async fn matches_insert_tiles(#[case] mbt_type: MbtType) {
        let tiles = tiles();
        let (mbt, mut expected) = schema(mbt_type).await;
        let batch: Vec<_> = tiles
            .iter()
            .map(|(z, x, y, data, _)| (*z, *x, *y, data.clone()))
            .collect();
        mbt.insert_tiles(&mut expected, mbt_type, CopyDuplicateMode::Abort, &batch)
            .await
            .unwrap();

        let (_, mut actual) = schema(mbt_type).await;
        bulk_write(
            &mbt,
            &mut actual,
            mbt_type,
            CopyDuplicateMode::Abort,
            &tiles,
        )
        .await
        .unwrap();

        assert_eq!(rows(&mut actual).await, rows(&mut expected).await);
        if mbt_type != MbtType::Cache {
            assert_eq!(
                mbt.update_agg_tiles_hash(&mut actual).await.unwrap(),
                mbt.update_agg_tiles_hash(&mut expected).await.unwrap()
            );
            mbt.validate(&mut actual, IntegrityCheckType::Full, AggHashType::Verify)
                .await
                .unwrap();
        }
    }

    #[rstest]
    #[case::hash_normalized(MbtType::Normalized { hash_view: false, schema: NormalizedSchema::Hash })]
    #[case::dedup_id(DEDUP_ID)]
    #[actix_rt::test]
    async fn stores_repeated_bytes_once(#[case] mbt_type: MbtType) {
        let (mbt, mut conn) = schema(mbt_type).await;
        let tiles = tiles();
        bulk_write(
            &mbt,
            &mut conn,
            mbt_type,
            CopyDuplicateMode::Override,
            &tiles,
        )
        .await
        .unwrap();
        let unique = tiles.iter().filter(|t| t.4 == TileDedup::Unique).count();
        // Plus one `fill` and one `same`.
        assert_eq!(blob_count(&mut conn, mbt_type).await, unique + 2);
    }

    async fn insert(
        mbt: &Mbtiles,
        conn: &mut SqliteConnection,
        (z, x, y): (u8, u32, u32),
        data: &[u8],
    ) {
        let batch = [(z, x, y, data.to_vec())];
        mbt.insert_tiles(conn, DEDUP_ID, CopyDuplicateMode::Override, &batch)
            .await
            .unwrap();
    }

    #[actix_rt::test]
    async fn dedup_id_shares_blobs_with_insert_tiles() {
        let (mbt, mut conn) = schema(DEDUP_ID).await;
        insert(&mbt, &mut conn, (1, 0, 0), b"a").await;
        // Appending: `a` is found by its hash, and new blobs continue the ids in use.
        let tiles = [
            (1, 1, 0, b"a".to_vec(), TileDedup::Unknown),
            (1, 0, 1, b"b".to_vec(), TileDedup::Unknown),
        ];
        bulk_write(
            &mbt,
            &mut conn,
            DEDUP_ID,
            CopyDuplicateMode::Override,
            &tiles,
        )
        .await
        .unwrap();
        let unique = [(1, 1, 1, b"u".to_vec(), TileDedup::Unique)];
        bulk_write(
            &mbt,
            &mut conn,
            DEDUP_ID,
            CopyDuplicateMode::Override,
            &unique,
        )
        .await
        .unwrap();
        // And `insert_tiles` finds the blob the writer stored by its hash.
        insert(&mbt, &mut conn, (2, 0, 0), b"b").await;
        assert_eq!(blob_count(&mut conn, DEDUP_ID).await, 3);
        let mut data: Vec<_> = rows(&mut conn).await.into_iter().map(|r| r.3).collect();
        data.sort();
        assert_eq!(data, [&b"a"[..], b"a", b"b", b"b", b"u"]);
    }

    #[rstest]
    #[case::override_(CopyDuplicateMode::Override, Some(&b"second"[..]))]
    #[case::ignore(CopyDuplicateMode::Ignore, Some(&b"first"[..]))]
    #[case::abort(CopyDuplicateMode::Abort, None)]
    #[actix_rt::test]
    async fn applies_the_duplicate_mode(
        #[case] mode: CopyDuplicateMode,
        #[case] expected: Option<&[u8]>,
        #[values(MbtType::Flat, DEDUP_ID)] mbt_type: MbtType,
    ) {
        let (mbt, mut conn) = schema(mbt_type).await;
        let tiles = [
            (0, 0, 0, b"first".to_vec(), TileDedup::Unique),
            (0, 0, 0, b"second".to_vec(), TileDedup::Unique),
        ];
        let result = bulk_write(&mbt, &mut conn, mbt_type, mode, &tiles).await;
        if let Some(data) = expected {
            result.unwrap();
            assert_eq!(rows(&mut conn).await, [(0, 0, 0, data.to_vec())]);
        } else {
            result.unwrap_err();
            assert_eq!(tile_count(&mut conn).await, 0);
        }
    }

    /// A caller's own error type, as `bulk_write` takes any that holds an [`MbtError`].
    #[derive(Debug)]
    enum Failure {
        Mbtiles(#[expect(dead_code, reason = "shown by Debug")] MbtError),
        Upstream,
    }

    impl From<MbtError> for Failure {
        fn from(err: MbtError) -> Self {
            Self::Mbtiles(err)
        }
    }

    #[actix_rt::test]
    async fn a_failed_write_keeps_only_committed_transactions() {
        let (mbt, mut conn) = schema(MbtType::Flat).await;
        let result: Result<(), Failure> = mbt
            .bulk_write(
                &mut conn,
                MbtType::Flat,
                CopyDuplicateMode::Abort,
                |writer| {
                    for i in 0..u32::try_from(DEFAULT_BATCH_SIZE.get()).unwrap() + 100 {
                        let coord = TileCoord::new_unchecked(9, i % 512, i / 512);
                        writer.write(coord, b"t", TileDedup::Unique)?;
                    }
                    Err(Failure::Upstream)
                },
            )
            .await;
        assert!(matches!(result, Err(Failure::Upstream)), "{result:?}");
        assert_eq!(tile_count(&mut conn).await, DEFAULT_BATCH_SIZE.get());
    }

    #[rstest]
    #[case::single_transaction(None, 0)]
    #[case::batches_of_three(NonZeroUsize::new(3), 6)]
    #[actix_rt::test]
    async fn batch_size_sets_what_a_failure_keeps(
        #[case] batch_size: Option<NonZeroUsize>,
        #[case] kept: usize,
    ) {
        let (mbt, mut conn) = schema(MbtType::Flat).await;
        let result: Result<(), Failure> = mbt
            .bulk_write(
                &mut conn,
                MbtType::Flat,
                CopyDuplicateMode::Abort,
                |writer| {
                    writer.batch_size = batch_size;
                    for x in 0..8 {
                        writer.write(TileCoord::new_unchecked(3, x, 0), b"t", TileDedup::Unique)?;
                    }
                    Err(Failure::Upstream)
                },
            )
            .await;
        assert!(matches!(result, Err(Failure::Upstream)), "{result:?}");
        assert_eq!(tile_count(&mut conn).await, kept);
    }

    /// A checkpoint commits what was written, and the write goes on as if it never happened.
    #[rstest]
    #[case::flat(MbtType::Flat)]
    #[case::hash_normalized(MbtType::Normalized { hash_view: false, schema: NormalizedSchema::Hash })]
    #[case::dedup_id(DEDUP_ID)]
    #[actix_rt::test]
    async fn checkpoints_commit_without_changing_the_result(#[case] mbt_type: MbtType) {
        let tiles = tiles();
        let checkpointed = |fail| {
            let tiles = &tiles;
            async move {
                let (mbt, mut conn) = schema(mbt_type).await;
                let result: Result<(), Failure> = mbt
                    .bulk_write(&mut conn, mbt_type, CopyDuplicateMode::Abort, |writer| {
                        for (i, (z, x, y, data, dedup)) in tiles.iter().enumerate() {
                            if i == tiles.len() / 2 && fail {
                                return Err(Failure::Upstream);
                            }
                            writer.write(TileCoord::new_unchecked(*z, *x, *y), data, *dedup)?;
                            if i % 5 == 4 {
                                writer.checkpoint()?;
                            }
                        }
                        Ok(())
                    })
                    .await;
                (result, conn)
            }
        };

        let (result, mut conn) = checkpointed(false).await;
        result.unwrap();
        let (mbt, mut expected) = schema(mbt_type).await;
        bulk_write(
            &mbt,
            &mut expected,
            mbt_type,
            CopyDuplicateMode::Abort,
            &tiles,
        )
        .await
        .unwrap();
        assert_eq!(rows(&mut conn).await, rows(&mut expected).await);
        if mbt_type != MbtType::Flat {
            assert_eq!(
                blob_count(&mut conn, mbt_type).await,
                blob_count(&mut expected, mbt_type).await
            );
        }

        let (result, mut conn) = checkpointed(true).await;
        assert!(matches!(result, Err(Failure::Upstream)), "{result:?}");
        assert_eq!(tile_count(&mut conn).await, tiles.len() / 2 / 5 * 5);
    }

    #[rstest]
    #[case::flat(MbtType::Flat)]
    #[case::flat_with_hash(MbtType::FlatWithHash)]
    #[case::hash_normalized(MbtType::Normalized { hash_view: false, schema: NormalizedSchema::Hash })]
    #[case::dedup_id(DEDUP_ID)]
    #[case::cache(MbtType::Cache)]
    #[actix_rt::test]
    async fn rejects_a_key_reused_for_other_bytes(#[case] mbt_type: MbtType) {
        let (mbt, mut conn) = schema(mbt_type).await;
        let tiles = [
            (0, 0, 0, b"fill".to_vec(), TileDedup::Key(1)),
            (1, 0, 0, b"lake".to_vec(), TileDedup::Key(1)),
        ];
        let err = bulk_write(&mbt, &mut conn, mbt_type, CopyDuplicateMode::Abort, &tiles)
            .await
            .unwrap_err();
        assert!(matches!(err, MbtError::DedupKeyMismatch(1)), "{err}");
        assert_eq!(tile_count(&mut conn).await, 0);
    }

    #[actix_rt::test]
    async fn holds_the_write_lock_from_the_start() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("locked.mbtiles");
        let mbt = Mbtiles::new(&path).unwrap();
        let mut conn = mbt.open_or_new().await.unwrap();
        init_mbtiles_schema(&mut conn, MbtType::Flat, false)
            .await
            .unwrap();
        // In WAL mode readers do not block writers, so only the write lock keeps others out.
        let other = Connection::open(&path).unwrap();
        other.pragma_update(None, "journal_mode", "WAL").unwrap();
        other.busy_timeout(std::time::Duration::ZERO).unwrap();
        mbt.bulk_write(&mut conn, MbtType::Flat, CopyDuplicateMode::Abort, |_| {
            let insert = "INSERT INTO tiles VALUES (0, 0, 0, x'00')";
            assert!(other.execute(insert, []).is_err(), "another writer got in");
            Ok::<_, MbtError>(())
        })
        .await
        .unwrap();
    }

    #[actix_rt::test]
    async fn action_with_rusqlite_rolls_back_a_transaction_left_open() {
        let (_, mut conn) = schema(MbtType::Flat).await;
        let err = action_with_rusqlite(&mut conn, |conn| {
            conn.execute_batch("BEGIN; INSERT INTO tiles VALUES (0, 0, 0, x'00')")?;
            Ok(())
        })
        .await
        .unwrap_err();
        assert!(matches!(err, MbtError::TransactionLeftOpen), "{err}");
        assert_eq!(tile_count(&mut conn).await, 0);

        // An action that failed keeps its own error.
        let err = action_with_rusqlite(&mut conn, |conn| {
            conn.execute_batch("BEGIN; INSERT INTO no_such_table VALUES (1)")?;
            Ok(())
        })
        .await
        .unwrap_err();
        assert!(matches!(err, MbtError::RusqliteError(_)), "{err}");
        assert_eq!(tile_count(&mut conn).await, 0);

        // A caller's own transaction is theirs to end.
        let mut tx = sqlx::Connection::begin(&mut conn).await.unwrap();
        action_with_rusqlite(&mut tx, |conn| {
            conn.execute_batch("INSERT INTO tiles VALUES (0, 0, 0, x'00')")?;
            Ok(())
        })
        .await
        .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(tile_count(&mut conn).await, 1);
    }

    #[test]
    fn ending_restores_synchronous_without_a_transaction() {
        // As after a periodic commit whose next `BEGIN IMMEDIATE` failed.
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "synchronous", "OFF").unwrap();
        end(&conn, "ROLLBACK", 2).unwrap_err();
        let synchronous: i64 = conn
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .unwrap();
        assert_eq!(synchronous, 2);
    }

    #[actix_rt::test]
    async fn restores_the_synchronous_setting() {
        async fn synchronous(conn: &mut SqliteConnection) -> i64 {
            let sql = query_scalar!("PRAGMA synchronous");
            sql.fetch_one(conn).await.unwrap().unwrap()
        }
        let (mbt, mut conn) = schema(MbtType::Flat).await;
        let before = synchronous(&mut conn).await;
        assert_ne!(before, 0);
        bulk_write(
            &mbt,
            &mut conn,
            MbtType::Flat,
            CopyDuplicateMode::Abort,
            &tiles(),
        )
        .await
        .unwrap();
        assert_eq!(synchronous(&mut conn).await, before);
        bulk_write(
            &mbt,
            &mut conn,
            MbtType::Flat,
            CopyDuplicateMode::Abort,
            &tiles(),
        )
        .await
        .unwrap_err();
        assert_eq!(synchronous(&mut conn).await, before);

        // Also when another connection holds the write lock, so the writer never begins.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("busy.mbtiles");
        let mbt = Mbtiles::new(&path).unwrap();
        let mut conn = mbt.open_or_new().await.unwrap();
        init_mbtiles_schema(&mut conn, MbtType::Flat, false)
            .await
            .unwrap();
        let sql = query!("PRAGMA busy_timeout = 0");
        sql.fetch_one(&mut conn).await.unwrap();
        let before = synchronous(&mut conn).await;
        let other = Connection::open(&path).unwrap();
        other.execute_batch("BEGIN IMMEDIATE").unwrap();
        bulk_write(
            &mbt,
            &mut conn,
            MbtType::Flat,
            CopyDuplicateMode::Abort,
            &tiles(),
        )
        .await
        .unwrap_err();
        assert_eq!(synchronous(&mut conn).await, before);
    }

    /// Only a file written from empty gives up syncing its commits.
    #[actix_rt::test]
    async fn keeps_syncing_a_file_that_has_tiles() {
        let (mbt, mut conn) = schema(MbtType::Flat).await;
        let before: i64 = query_scalar!("PRAGMA synchronous")
            .fetch_one(&mut conn)
            .await
            .unwrap()
            .unwrap();
        let mut during = Vec::<i64>::new();
        for x in 0..2 {
            mbt.bulk_write(
                &mut conn,
                MbtType::Flat,
                CopyDuplicateMode::Abort,
                |writer| {
                    let synchronous = writer
                        .conn
                        .pragma_query_value(None, "synchronous", |row| row.get(0));
                    during.push(synchronous?);
                    writer.write(TileCoord::new_unchecked(1, x, 0), b"t", TileDedup::Unique)
                },
            )
            .await
            .unwrap();
        }
        assert_eq!(during, [0, before]);
    }

    #[actix_rt::test]
    async fn dedup_id_hash_index_covers_hinted_blobs() {
        let (mbt, mut conn) = schema(DEDUP_ID).await;
        let tile = |x, data: &[u8], dedup| (4, x, 0, data.to_vec(), dedup);
        // Hinted blobs first: the index created for `Unknown` tiles includes them.
        let tiles = [
            tile(0, b"k", TileDedup::Key(1)),
            tile(1, b"u", TileDedup::Unique),
            tile(2, b"k", TileDedup::Unknown),
            tile(3, b"u", TileDedup::Unknown),
        ];
        bulk_write(&mbt, &mut conn, DEDUP_ID, CopyDuplicateMode::Abort, &tiles)
            .await
            .unwrap();
        assert_eq!(blob_count(&mut conn, DEDUP_ID).await, 2);
        // With the index in use, hinted blobs are found in it and recorded there.
        let tiles = [
            tile(4, b"k", TileDedup::Key(9)),
            tile(5, b"y", TileDedup::Unique),
            tile(6, b"y", TileDedup::Unknown),
        ];
        bulk_write(&mbt, &mut conn, DEDUP_ID, CopyDuplicateMode::Abort, &tiles)
            .await
            .unwrap();
        insert(&mbt, &mut conn, (4, 7, 0), b"y").await;
        assert_eq!(blob_count(&mut conn, DEDUP_ID).await, 3);
        assert_eq!(tile_count(&mut conn).await, 8);
    }

    #[actix_rt::test]
    async fn hashes_with_the_files_algorithm() {
        let (mbt, mut conn) = schema(MbtType::FlatWithHash).await;
        mbt.set_metadata_value(&mut conn, HASH_ALGORITHM, HashAlgorithm::Xxh3.as_str())
            .await
            .unwrap();
        bulk_write(
            &mbt,
            &mut conn,
            MbtType::FlatWithHash,
            CopyDuplicateMode::Abort,
            &tiles(),
        )
        .await
        .unwrap();
        let sql = query_scalar!("SELECT tile_hash FROM tiles_with_hash LIMIT 1");
        let hash = sql.fetch_one(&mut conn).await.unwrap().unwrap();
        assert_eq!(hash.len(), 16, "an xxh3 hash, not md5: {hash}");
        mbt.update_agg_tiles_hash(&mut conn).await.unwrap();
        mbt.validate(&mut conn, IntegrityCheckType::Full, AggHashType::Verify)
            .await
            .unwrap();
    }
}

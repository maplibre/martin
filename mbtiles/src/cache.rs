//! Use an `.mbtiles` (`SQLite`) file as a persistent, de-duplicating tile cache.
//!
//! This is a **non-standard** schema (not part of the `MBTiles` specification) built on
//! top of the same `SQLite` file format. It stores tiles together with cache metadata
//! (`expires` and `etag`) and de-duplicates identical tile blobs, and it is intended to
//! be embedded by other systems that need a simple on-disk tile cache.
//!
//! # Schemas
//!
//! Two layouts are supported, chosen via [`CacheSchema`]. Both center on a `tile_cache`
//! table storing tile coordinates with `expires`/`etag` metadata, and both expose a
//! spec-compatible `tiles` view so the file can still be opened by any standard
//! `MBTiles` reader (the `expires`/`etag` columns are simply invisible to it).
//!
//! - [`CacheSchema::Flat`]: `tile_cache(zoom_level, tile_column, tile_row, expires,
//!   etag, tile_data)` - the blob is stored inline. Simple and fast, best when few
//!   tiles share content.
//! - [`CacheSchema::Normalized`]: `tile_cache(zoom_level, tile_column, tile_row,
//!   expires, etag, tile_id)` (`WITHOUT ROWID`) plus `cache_data(tile_id INTEGER
//!   PRIMARY KEY, tile_data BLOB)`. `tile_id` is the
//!   [xxh3-64](https://github.com/Cyan4973/xxHash) hash of `tile_data`, stored as an
//!   `INTEGER PRIMARY KEY` so it aliases the rowid (single B-tree, no secondary index).
//!   Identical blobs collapse to one row - the best default for web-tile caches, where
//!   identical (e.g. empty or ocean) tiles are common.
//!
//! Coordinates use the XYZ (Slippy map) scheme on the API, matching the rest of the crate;
//! the TMS `tile_row` inversion is handled internally.
//!
//! # Negative caching
//!
//! An **empty blob** is the convention for a cached negative response (e.g. an upstream
//! HTTP `404`/`204`): [`Mbtiles::get_cached`] returning `Some` with empty
//! [`CachedTile::data`] means "cached as absent" (with its own `expires`/`etag`),
//! while `None` means "not in the cache at all". Empty blobs de-duplicate into a single
//! `cache_data` row like any other content.
//!
//! # Bulk copies vs. runtime writes (normalized layout)
//!
//! Only [`Mbtiles::set_cached`] resolves xxh3-64 collisions (by linear probing). The bulk
//! SQL paths - `mbtiles copy` into a normalized cache file and [`Mbtiles::insert_tiles`] -
//! key blobs with `INSERT OR IGNORE` instead: the copier detects a collision afterwards and
//! fails with [`MbtError::CacheCopyCollision`], while `insert_tiles` cannot detect one (the
//! source bytes are not in a table to compare against) and accepts the ~2⁻⁶⁴ risk.
//! The flat layout stores blobs inline and has no collision concerns.
//!
//! See [`crate::MbtilesCache`] for a pooled, writable entry point.

use sqlx::{Connection as _, Row as _, SqliteConnection, SqliteExecutor, query, query_scalar};
use tracing::debug;
use xxhash_rust::xxh3::xxh3_64;

use crate::errors::MbtResult;
use crate::queries::create_metadata_table;
use crate::schemas::{cache_tables_schema, create_cache_tables};
use crate::{CacheSchema, MbtError, Mbtiles, invert_y_value};

/// Maximum number of linear probes when resolving an xxh3-64 collision in `set_cached`
/// before giving up with [`MbtError::CacheKeyExhausted`]. Collisions require billions of
/// distinct blobs to even begin, so this is only a safety valve against pathological input.
/// Also the tolerance window used by per-tile validation of cache files.
pub(crate) const MAX_KEY_PROBES: u32 = 1024;

/// A cached tile together with its cache metadata, as returned by [`Mbtiles::get_cached`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedTile {
    /// The tile blob.
    pub data: Vec<u8>,
    /// Unix-epoch (seconds) expiration time, or `None` if the entry never expires.
    ///
    /// The value is returned exactly as stored; the cache does **not** filter out expired
    /// entries. Callers decide how to treat them (e.g. serve-stale-while-revalidate using
    /// [`CachedTile::etag`], or refetch).
    pub expires: Option<i64>,
    /// Upstream validator (e.g. an HTTP `ETag`) stored with the tile, if any.
    pub etag: Option<String>,
}

/// Cache metadata attached to a tile when writing it with [`Mbtiles::set_cached`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheEntryMeta<'a> {
    /// Unix-epoch (seconds) expiration time, or `None` for an entry that never expires.
    pub expires: Option<i64>,
    /// Upstream validator (e.g. an HTTP `ETag`), or `None`.
    pub etag: Option<&'a str>,
}

impl<'a> CacheEntryMeta<'a> {
    /// Create cache metadata with both an `expires` (Unix-epoch seconds) and an `etag`.
    ///
    /// For entries missing one or both, construct the struct directly (its fields are
    /// public) or use [`CacheEntryMeta::default`] for "never expires, no etag".
    #[must_use]
    pub fn new(expires: i64, etag: &'a str) -> Self {
        Self {
            expires: Some(expires),
            etag: Some(etag),
        }
    }
}

/// Compute the content key (xxh3-64) used as the primary key of the `cache_data` table.
///
/// xxh3 is unsigned but `SQLite` rowids are signed, so we reinterpret the bits (rather than
/// numerically cast) to keep a lossless, round-trippable 64-bit key. The `xxh3_64_int`
/// SQL function registered by `attach_sqlite_fn` MUST stay identical to this.
#[must_use]
pub(crate) fn content_key(data: &[u8]) -> i64 {
    i64::from_ne_bytes(xxh3_64(data).to_ne_bytes())
}

impl Mbtiles {
    /// Create the tile-cache schema of the given [`CacheSchema`] layout (`metadata` and
    /// the cache tables plus the `tiles` view) if it does not already exist.
    ///
    /// Pass `strict = true` to create `STRICT` tables.
    pub async fn create_cache_schema<T>(
        &self,
        conn: &mut T,
        schema: CacheSchema,
        strict: bool,
    ) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        create_metadata_table(&mut *conn, strict).await?;
        create_cache_tables(&mut *conn, schema, strict).await
    }

    /// Returns the [`CacheSchema`] layout of this file, or `None` if it does not use a
    /// tile-cache schema.
    pub async fn cache_schema<T>(&self, conn: &mut T) -> MbtResult<Option<CacheSchema>>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        cache_tables_schema(conn).await
    }

    /// Returns `true` if this file uses one of the tile-cache schemas.
    pub async fn is_cache<T>(&self, conn: &mut T) -> MbtResult<bool>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        Ok(cache_tables_schema(conn).await?.is_some())
    }

    /// Look up a cached tile by its XYZ coordinates.
    ///
    /// Returns the tile together with its `expires`/`etag` metadata, or `None` if there is
    /// no entry at the given coordinates. Expired entries are still returned (with their
    /// stored `expires`) so the caller can decide whether to serve stale, revalidate via
    /// `etag`, or refetch.
    pub async fn get_cached<T>(
        &self,
        conn: &mut T,
        schema: CacheSchema,
        z: u8,
        x: u32,
        y: u32,
    ) -> MbtResult<Option<CachedTile>>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let sql = match schema {
            CacheSchema::Flat => {
                "SELECT tile_data, expires, etag
                 FROM tile_cache
                 WHERE zoom_level = ?1 AND tile_column = ?2 AND tile_row = ?3"
            }
            CacheSchema::Normalized => {
                "SELECT d.tile_data, c.expires, c.etag
                 FROM tile_cache c
                 JOIN cache_data d ON d.tile_id = c.tile_id
                 WHERE c.zoom_level = ?1 AND c.tile_column = ?2 AND c.tile_row = ?3"
            }
        };
        let row = query(sql)
            .bind(z)
            .bind(x)
            .bind(invert_y_value(z, y))
            .fetch_optional(conn)
            .await?;

        Ok(row.map(|row| CachedTile {
            data: row.get(0),
            expires: row.get(1),
            etag: row.get(2),
        }))
    }

    /// Insert or replace a cached tile, with its [`CacheEntryMeta`] (`expires`/`etag`).
    ///
    /// With [`CacheSchema::Flat`], this is a plain upsert with the blob stored inline.
    ///
    /// With [`CacheSchema::Normalized`], the tile blob is de-duplicated by content:
    /// identical blobs share a single `cache_data` row keyed on their xxh3-64 hash.
    /// On the astronomically-rare hash collision (a *different* blob already stored under the
    /// hash key), the key is resolved by **linear probing** - `key`, `key + 1`, … - until a
    /// free slot or a slot holding identical bytes is found. The resolved key is stored in the
    /// index row, so reads never probe and no existing entry is ever overwritten. If no slot
    /// is found within [`MAX_KEY_PROBES`], returns [`MbtError::CacheKeyExhausted`].
    #[expect(clippy::too_many_arguments)]
    pub async fn set_cached(
        &self,
        conn: &mut SqliteConnection,
        schema: CacheSchema,
        z: u8,
        x: u32,
        y: u32,
        data: &[u8],
        meta: CacheEntryMeta<'_>,
    ) -> MbtResult<()> {
        if schema == CacheSchema::Flat {
            // Inline blob: a plain upsert, no de-duplication or key probing involved.
            query(
                "INSERT OR REPLACE INTO tile_cache
                     (zoom_level, tile_column, tile_row, expires, etag, tile_data)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .bind(z)
            .bind(x)
            .bind(invert_y_value(z, y))
            .bind(meta.expires)
            .bind(meta.etag)
            .bind(data)
            .execute(&mut *conn)
            .await?;
            return Ok(());
        }

        let mut tx = conn.begin().await?;

        // Resolve the content key. Reuse an existing slot holding identical bytes, or claim a
        // free slot; on a collision (slot occupied by *different* bytes) probe the next key.
        let mut key = content_key(data);
        let mut probes = 0u32;
        let resolved = loop {
            // `INSERT ... ON CONFLICT DO NOTHING` is the atomic claim: rows_affected == 1
            // means we claimed a free slot for our bytes (race-safe under concurrent writers).
            let claimed = query(
                "INSERT INTO cache_data (tile_id, tile_data) VALUES (?1, ?2)
                 ON CONFLICT(tile_id) DO NOTHING",
            )
            .bind(key)
            .bind(data)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if claimed == 1 {
                break key;
            }

            // Slot occupied: reuse it iff it holds identical bytes (a de-dup hit).
            let same = query("SELECT 1 FROM cache_data WHERE tile_id = ?1 AND tile_data = ?2")
                .bind(key)
                .bind(data)
                .fetch_optional(&mut *tx)
                .await?
                .is_some();
            if same {
                break key;
            }

            if probes >= MAX_KEY_PROBES {
                return Err(MbtError::CacheKeyExhausted { z, x, y, probes });
            }
            if probes == 0 {
                debug!(
                    "xxh3-64 collision for cached tile {z}/{x}/{y} in {self}; probing for a free slot"
                );
            }
            probes += 1;
            key = key.wrapping_add(1);
        };

        query(
            "INSERT INTO tile_cache (zoom_level, tile_column, tile_row, expires, etag, tile_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(zoom_level, tile_column, tile_row)
             DO UPDATE SET expires = excluded.expires,
                           etag = excluded.etag,
                           tile_id = excluded.tile_id",
        )
        .bind(z)
        .bind(x)
        .bind(invert_y_value(z, y))
        .bind(meta.expires)
        .bind(meta.etag)
        .bind(resolved)
        .execute(&mut *tx)
        .await?;
        // If this overwrote an entry pointing at a different blob, that blob may now be
        // an orphaned `cache_data` row. It is pruned at a convenient/idle time by
        // `purge_expired` / `purge_cache_to_size`.

        tx.commit().await?;
        Ok(())
    }

    /// Update only the `expires`/`etag` metadata of an existing cache entry, without
    /// touching the tile blob.
    ///
    /// This is the revalidation path: after a conditional refetch (e.g. HTTP
    /// `If-None-Match` answered with `304 Not Modified`), the cached bytes are still
    /// valid and only the freshness metadata needs a bump.
    ///
    /// Returns `true` if an entry existed at the given coordinates and was updated, and
    /// `false` if there is no such entry (e.g. it was purged concurrently) - the caller
    /// should then store the full tile with [`Mbtiles::set_cached`].
    pub async fn update_cached_meta<T>(
        &self,
        conn: &mut T,
        z: u8,
        x: u32,
        y: u32,
        meta: CacheEntryMeta<'_>,
    ) -> MbtResult<bool>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let updated = query(
            "UPDATE tile_cache SET expires = ?4, etag = ?5
             WHERE zoom_level = ?1 AND tile_column = ?2 AND tile_row = ?3",
        )
        .bind(z)
        .bind(x)
        .bind(invert_y_value(z, y))
        .bind(meta.expires)
        .bind(meta.etag)
        .execute(conn)
        .await?
        .rows_affected();
        Ok(updated > 0)
    }

    /// Delete all entries whose `expires` timestamp is strictly less than `now` (a Unix-epoch
    /// seconds value), then remove any blobs that are no longer referenced
    /// ([`CacheSchema::Normalized`] only - the flat layout has no separate blobs).
    ///
    /// Returns the number of `tile_cache` rows removed. Entries with `expires IS NULL` never
    /// expire and are left untouched. Freed pages are released back to the OS via
    /// `PRAGMA incremental_vacuum`, which only shrinks the file when it has
    /// `auto_vacuum` enabled (files created by [`crate::MbtilesCache::open`] do); for other
    /// files the pages are reused by later writes, and a one-off full `VACUUM` is needed
    /// to shrink them on disk.
    pub async fn purge_expired(
        &self,
        conn: &mut SqliteConnection,
        schema: CacheSchema,
        now: i64,
    ) -> MbtResult<u64> {
        let mut tx = conn.begin().await?;
        let removed = query("DELETE FROM tile_cache WHERE expires IS NOT NULL AND expires < ?1")
            .bind(now)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        gc_orphaned_blobs(&mut tx, schema).await?;
        tx.commit().await?;
        query("PRAGMA incremental_vacuum")
            .execute(&mut *conn)
            .await?;
        Ok(removed)
    }

    /// Evict cache entries until the database's live size is at most `max_bytes`.
    ///
    /// Entries are evicted soonest-expiring first (`expires` ascending); never-expiring
    /// entries (`expires IS NULL`) are evicted last. Returns the number of `tile_cache`
    /// rows removed.
    ///
    /// "Live size" is the file size minus free pages (`page_count - freelist_count`
    /// times `page_size`). The same `PRAGMA incremental_vacuum` note as
    /// [`Mbtiles::purge_expired`] applies: the file only shrinks on disk when it has
    /// `auto_vacuum` enabled. An empty cache still has a fixed schema overhead, so
    /// tiny `max_bytes` values evict everything without reaching the target.
    pub async fn purge_cache_to_size(
        &self,
        conn: &mut SqliteConnection,
        schema: CacheSchema,
        max_bytes: u64,
    ) -> MbtResult<u64> {
        /// How many `tile_cache` rows to evict between size re-measurements. Small enough
        /// to not massively overshoot the target, large enough to amortize the PRAGMAs.
        const EVICT_CHUNK: u32 = 64;
        let mut removed = 0;
        while db_live_size(&mut *conn).await? > max_bytes {
            let mut tx = conn.begin().await?;
            let evicted = query(
                "DELETE FROM tile_cache WHERE (zoom_level, tile_column, tile_row) IN
                     (SELECT zoom_level, tile_column, tile_row FROM tile_cache
                      ORDER BY expires IS NULL, expires LIMIT ?1)",
            )
            .bind(EVICT_CHUNK)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            gc_orphaned_blobs(&mut tx, schema).await?;
            tx.commit().await?;
            query("PRAGMA incremental_vacuum")
                .execute(&mut *conn)
                .await?;
            removed += evicted;
            if evicted == 0 {
                break; // the cache is empty; what remains is fixed schema overhead
            }
        }
        Ok(removed)
    }
}

/// Remove `cache_data` blobs no longer referenced by any `tile_cache` entry
/// ([`CacheSchema::Normalized`] only - the flat layout stores blobs inline).
async fn gc_orphaned_blobs(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    schema: CacheSchema,
) -> MbtResult<()> {
    if schema == CacheSchema::Normalized {
        query("DELETE FROM cache_data WHERE tile_id NOT IN (SELECT tile_id FROM tile_cache)")
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// The database size excluding free pages: `(page_count - freelist_count) * page_size`.
async fn db_live_size(conn: &mut SqliteConnection) -> MbtResult<u64> {
    let page_count: i64 = query_scalar("PRAGMA page_count")
        .fetch_one(&mut *conn)
        .await?;
    let freelist: i64 = query_scalar("PRAGMA freelist_count")
        .fetch_one(&mut *conn)
        .await?;
    let page_size: i64 = query_scalar("PRAGMA page_size")
        .fetch_one(&mut *conn)
        .await?;
    let live_pages = u64::try_from((page_count - freelist).max(0)).expect("value is non-negative");
    let page_size = u64::try_from(page_size.max(0)).expect("value is non-negative");
    Ok(live_pages * page_size)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::CacheSchema::{Flat, Normalized};
    use crate::{CacheEntryMeta, CacheSchema, Mbtiles};

    /// Open an in-memory cache file with the given layout created.
    async fn cache(schema: CacheSchema) -> (Mbtiles, sqlx::SqliteConnection) {
        let mbt = Mbtiles::new(":memory:").unwrap();
        let mut conn = mbt.open().await.unwrap();
        mbt.create_cache_schema(&mut conn, schema, false)
            .await
            .unwrap();
        (mbt, conn)
    }

    /// Count the rows of the table holding tile blobs in the given layout.
    async fn blob_count(conn: &mut sqlx::SqliteConnection, schema: CacheSchema) -> i64 {
        let sql = match schema {
            Flat => "SELECT COUNT(*) FROM tile_cache",
            Normalized => "SELECT COUNT(*) FROM cache_data",
        };
        sqlx::query_scalar(sql).fetch_one(conn).await.unwrap()
    }

    #[rstest]
    #[tokio::test]
    async fn detected_as_cache(#[values(Flat, Normalized)] schema: CacheSchema) {
        let (mbt, mut conn) = cache(schema).await;
        assert_eq!(mbt.cache_schema(&mut conn).await.unwrap(), Some(schema));
        assert!(mbt.is_cache(&mut conn).await.unwrap());
        assert_eq!(
            mbt.detect_type(&mut conn).await.unwrap(),
            crate::MbtType::Cache { schema }
        );
    }

    #[rstest]
    #[tokio::test]
    async fn set_get_roundtrip(#[values(Flat, Normalized)] schema: CacheSchema) {
        let (mbt, mut conn) = cache(schema).await;
        assert!(
            mbt.get_cached(&mut conn, schema, 3, 1, 2)
                .await
                .unwrap()
                .is_none()
        );

        mbt.set_cached(
            &mut conn,
            schema,
            3,
            1,
            2,
            b"hello",
            CacheEntryMeta::new(100, "etag-1"),
        )
        .await
        .unwrap();
        let got = mbt
            .get_cached(&mut conn, schema, 3, 1, 2)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.data, b"hello");
        assert_eq!(got.expires, Some(100));
        assert_eq!(got.etag.as_deref(), Some("etag-1"));

        // Overwrite the same coordinate with new data/metadata.
        mbt.set_cached(
            &mut conn,
            schema,
            3,
            1,
            2,
            b"world",
            CacheEntryMeta::default(),
        )
        .await
        .unwrap();
        let got = mbt
            .get_cached(&mut conn, schema, 3, 1, 2)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.data, b"world");
        assert_eq!(got.expires, None);
        assert_eq!(got.etag, None);
    }

    #[tokio::test]
    async fn blob_is_deduplicated() {
        let (mbt, mut conn) = cache(Normalized).await;
        mbt.set_cached(
            &mut conn,
            Normalized,
            0,
            0,
            0,
            b"same",
            CacheEntryMeta::default(),
        )
        .await
        .unwrap();
        mbt.set_cached(
            &mut conn,
            Normalized,
            1,
            0,
            0,
            b"same",
            CacheEntryMeta::default(),
        )
        .await
        .unwrap();

        assert_eq!(
            blob_count(&mut conn, Normalized).await,
            1,
            "identical blobs should share one cache_data row"
        );
        let entries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tile_cache")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(entries, 2);
    }

    #[tokio::test]
    async fn flat_stores_blobs_inline() {
        let (mbt, mut conn) = cache(Flat).await;
        mbt.set_cached(&mut conn, Flat, 0, 0, 0, b"same", CacheEntryMeta::default())
            .await
            .unwrap();
        mbt.set_cached(&mut conn, Flat, 1, 0, 0, b"same", CacheEntryMeta::default())
            .await
            .unwrap();

        // No de-duplication: each entry stores its own copy of the blob
        assert_eq!(blob_count(&mut conn, Flat).await, 2);
    }

    #[tokio::test]
    async fn collision_probes_next_slot_without_corrupting() {
        let (mbt, mut conn) = cache(Normalized).await;
        let victim: &[u8] = b"the-real-tile";
        let key = super::content_key(victim);

        // Pre-occupy the victim's hash slot with DIFFERENT bytes (simulated collision).
        let squatter: &[u8] = b"different-bytes-same-key";
        sqlx::query("INSERT INTO cache_data (tile_id, tile_data) VALUES (?1, ?2)")
            .bind(key)
            .bind(squatter)
            .execute(&mut conn)
            .await
            .unwrap();

        // Writing the victim must NOT overwrite the squatter; it probes to key+1.
        mbt.set_cached(
            &mut conn,
            Normalized,
            4,
            3,
            2,
            victim,
            CacheEntryMeta::default(),
        )
        .await
        .unwrap();
        assert_eq!(
            mbt.get_cached(&mut conn, Normalized, 4, 3, 2)
                .await
                .unwrap()
                .unwrap()
                .data,
            victim
        );

        // Squatter at `key` is untouched; victim lives at key+1.
        let at_key: Vec<u8> =
            sqlx::query_scalar("SELECT tile_data FROM cache_data WHERE tile_id = ?1")
                .bind(key)
                .fetch_one(&mut conn)
                .await
                .unwrap();
        assert_eq!(at_key, squatter);
        let at_next: Vec<u8> =
            sqlx::query_scalar("SELECT tile_data FROM cache_data WHERE tile_id = ?1")
                .bind(key.wrapping_add(1))
                .fetch_one(&mut conn)
                .await
                .unwrap();
        assert_eq!(at_next, victim);

        // Re-writing the same victim to another coord reuses key+1 (dedup after probe);
        // it must not create a third blob row.
        mbt.set_cached(
            &mut conn,
            Normalized,
            5,
            1,
            1,
            victim,
            CacheEntryMeta::default(),
        )
        .await
        .unwrap();
        assert_eq!(
            blob_count(&mut conn, Normalized).await,
            2,
            "squatter + victim only"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn readable_via_tiles_view(#[values(Flat, Normalized)] schema: CacheSchema) {
        let (mbt, mut conn) = cache(schema).await;
        // z=1, y=0 (XYZ) => TMS tile_row = 1
        mbt.set_cached(
            &mut conn,
            schema,
            1,
            0,
            0,
            b"viewdata",
            CacheEntryMeta::default(),
        )
        .await
        .unwrap();
        let data = mbt.get_tile(&mut conn, 1, 0, 0).await.unwrap().unwrap();
        assert_eq!(data, b"viewdata");
    }

    #[rstest]
    #[tokio::test]
    async fn purge_expired_removes_and_gcs(#[values(Flat, Normalized)] schema: CacheSchema) {
        let (mbt, mut conn) = cache(schema).await;
        let stale = CacheEntryMeta {
            expires: Some(50),
            etag: None,
        };
        mbt.set_cached(&mut conn, schema, 0, 0, 0, b"stale", stale)
            .await
            .unwrap();
        let fresh = CacheEntryMeta {
            expires: Some(200),
            etag: None,
        };
        mbt.set_cached(&mut conn, schema, 1, 0, 0, b"fresh", fresh)
            .await
            .unwrap();
        mbt.set_cached(
            &mut conn,
            schema,
            1,
            1,
            0,
            b"forever",
            CacheEntryMeta::default(),
        )
        .await
        .unwrap();

        let removed = mbt.purge_expired(&mut conn, schema, 100).await.unwrap();
        assert_eq!(removed, 1);

        assert!(
            mbt.get_cached(&mut conn, schema, 0, 0, 0)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            mbt.get_cached(&mut conn, schema, 1, 0, 0)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            mbt.get_cached(&mut conn, schema, 1, 1, 0)
                .await
                .unwrap()
                .is_some()
        );

        // The purged tile's blob is gone too (GC'd from cache_data for normalized,
        // deleted with its row for flat).
        assert_eq!(blob_count(&mut conn, schema).await, 2);
    }

    #[rstest]
    #[tokio::test]
    async fn purge_gcs_orphan_from_overwrite(#[values(Flat, Normalized)] schema: CacheSchema) {
        let (mbt, mut conn) = cache(schema).await;
        mbt.set_cached(
            &mut conn,
            schema,
            2,
            1,
            1,
            b"old",
            CacheEntryMeta::default(),
        )
        .await
        .unwrap();
        mbt.set_cached(
            &mut conn,
            schema,
            2,
            1,
            1,
            b"new",
            CacheEntryMeta::default(),
        )
        .await
        .unwrap();
        // Normalized: the overwrite re-pointed the entry, orphaning the "old" blob.
        // Flat: the overwrite replaced the row in place, so there is nothing to orphan.
        let expected_before = match schema {
            Flat => 1,
            Normalized => 2,
        };
        assert_eq!(blob_count(&mut conn, schema).await, expected_before);

        // Nothing is expired, so no entries are removed - but the orphan is GC'd.
        assert_eq!(mbt.purge_expired(&mut conn, schema, 100).await.unwrap(), 0);
        assert_eq!(blob_count(&mut conn, schema).await, 1);
        let got = mbt
            .get_cached(&mut conn, schema, 2, 1, 1)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.data, b"new");
    }

    #[rstest]
    #[tokio::test]
    async fn update_meta_without_rewriting_blob(#[values(Flat, Normalized)] schema: CacheSchema) {
        let (mbt, mut conn) = cache(schema).await;
        mbt.set_cached(
            &mut conn,
            schema,
            3,
            1,
            2,
            b"payload",
            CacheEntryMeta::new(100, "etag-1"),
        )
        .await
        .unwrap();

        // No entry at these coordinates - the caller must fall back to set_cached.
        let missing = CacheEntryMeta::new(1, "x");
        assert!(
            !mbt.update_cached_meta(&mut conn, 3, 0, 0, missing)
                .await
                .unwrap()
        );

        // Revalidation bumps the metadata in place; the blob stays untouched.
        let bumped = CacheEntryMeta::new(500, "etag-2");
        assert!(
            mbt.update_cached_meta(&mut conn, 3, 1, 2, bumped)
                .await
                .unwrap()
        );
        let got = mbt
            .get_cached(&mut conn, schema, 3, 1, 2)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.data, b"payload");
        assert_eq!(got.expires, Some(500));
        assert_eq!(got.etag.as_deref(), Some("etag-2"));
        assert_eq!(blob_count(&mut conn, schema).await, 1);
    }

    #[rstest]
    #[tokio::test]
    async fn empty_blob_caches_negative_response(#[values(Flat, Normalized)] schema: CacheSchema) {
        let (mbt, mut conn) = cache(schema).await;
        assert!(
            mbt.get_cached(&mut conn, schema, 5, 1, 1)
                .await
                .unwrap()
                .is_none()
        );

        // Cached negative: Some(empty) with its own freshness metadata.
        mbt.set_cached(
            &mut conn,
            schema,
            5,
            1,
            1,
            b"",
            CacheEntryMeta::new(60, "miss-etag"),
        )
        .await
        .unwrap();
        let got = mbt
            .get_cached(&mut conn, schema, 5, 1, 1)
            .await
            .unwrap()
            .unwrap();
        assert!(got.data.is_empty());
        assert_eq!(got.expires, Some(60));
        assert_eq!(got.etag.as_deref(), Some("miss-etag"));

        // Normalized de-duplicates empty blobs like any other content; flat stores each.
        mbt.set_cached(&mut conn, schema, 5, 2, 2, b"", CacheEntryMeta::default())
            .await
            .unwrap();
        let expected = match schema {
            Flat => 2,
            Normalized => 1,
        };
        assert_eq!(blob_count(&mut conn, schema).await, expected);
    }

    #[rstest]
    #[tokio::test]
    async fn insert_tiles_bulk(#[values(Flat, Normalized)] schema: CacheSchema) {
        let (mbt, mut conn) = cache(schema).await;
        let batch: Vec<(u8, u32, u32, Vec<u8>)> = vec![
            (1, 0, 0, b"same".to_vec()),
            (1, 1, 0, b"same".to_vec()),
            (1, 1, 1, b"other".to_vec()),
        ];
        mbt.insert_tiles(
            &mut conn,
            crate::MbtType::Cache { schema },
            crate::CopyDuplicateMode::Override,
            &batch,
        )
        .await
        .unwrap();

        // Normalized de-duplicates the two identical blobs; flat stores all three
        let expected = match schema {
            Flat => 3,
            Normalized => 2,
        };
        assert_eq!(blob_count(&mut conn, schema).await, expected);
        let got = mbt
            .get_cached(&mut conn, schema, 1, 1, 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.data, b"same");
        assert_eq!(got.expires, None, "bulk-inserted tiles never expire");
        assert_eq!(got.etag, None);
    }

    #[rstest]
    #[tokio::test]
    async fn purge_to_size_evicts_expiring_first(#[values(Flat, Normalized)] schema: CacheSchema) {
        let (mbt, mut conn) = cache(schema).await;
        // 80 expiring + 20 never-expiring tiles, each a distinct 8 KiB blob (larger than
        // a page, so evictions free their overflow pages immediately).
        for i in 0..100u32 {
            let data = vec![u8::try_from(i % 251).unwrap(); 8192];
            let meta = if i < 80 {
                CacheEntryMeta {
                    expires: Some(i64::from(i)),
                    etag: None,
                }
            } else {
                CacheEntryMeta::default()
            };
            mbt.set_cached(&mut conn, schema, 9, i, 0, &data, meta)
                .await
                .unwrap();
        }

        let initial = super::db_live_size(&mut conn).await.unwrap();
        let budget = initial - 300 * 1024;
        let removed = mbt
            .purge_cache_to_size(&mut conn, schema, budget)
            .await
            .unwrap();
        assert!(removed > 0);
        assert!(super::db_live_size(&mut conn).await.unwrap() <= budget);

        // Soonest-expiring entries went first; never-expiring ones survived.
        assert!(
            mbt.get_cached(&mut conn, schema, 9, 0, 0)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            mbt.get_cached(&mut conn, schema, 9, 99, 0)
                .await
                .unwrap()
                .is_some()
        );

        // Already under budget: a second call is a no-op.
        assert_eq!(
            mbt.purge_cache_to_size(&mut conn, schema, budget)
                .await
                .unwrap(),
            0
        );
    }
}

//! Use an `.mbtiles` (`SQLite`) file as a persistent tile cache.
//!
//! This is a **non-standard** schema (not part of the `MBTiles` specification) built on
//! top of the same `SQLite` file format. It stores tiles together with cache metadata
//! (`fetched`, `expires`, and `etag`), and it is intended to be embedded by other
//! systems that need a simple on-disk tile cache.
//!
//! # Schema
//!
//! - `tile_cache(zoom_level, tile_column, tile_row, fetched, expires, etag, tile_data)` -
//!   tile coordinates with the cache metadata and the tile blob stored inline, clustered
//!   on `(zoom_level, tile_column, tile_row)`.
//! - `tiles` view - a spec-compatible read view, so the file can still be opened by any
//!   standard `MBTiles` reader (the extra cache columns are simply invisible to it).
//!
//! Coordinates use the XYZ (Slippy map) scheme on the API, matching the rest of the crate;
//! the TMS `tile_row` inversion is handled internally.
//!
//! # Negative caching
//!
//! An **empty blob** is the convention for a cached negative response (e.g. an upstream
//! HTTP `404`/`204`): [`Mbtiles::get_cached`] returning `Some` with empty
//! [`CachedTile::data`] means "cached as absent" (with its own metadata), while `None`
//! means "not in the cache at all".
//!
//! See [`crate::MbtilesCache`] for a pooled, writable entry point.

use sqlx::{Row as _, SqliteConnection, SqliteExecutor, query, query_scalar};

use crate::errors::MbtResult;
use crate::queries::create_metadata_table;
use crate::schemas::{create_cache_tables, is_cache_tables_type};
use crate::{Mbtiles, invert_y_value};

/// A cached tile together with its cache metadata, as returned by [`Mbtiles::get_cached`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedTile {
    /// The tile blob.
    pub data: Vec<u8>,
    /// Unix-epoch (seconds) time the tile was downloaded/added/last refreshed, or `None`
    /// if unknown (e.g. the entry was bulk-imported with `mbtiles copy`).
    pub fetched: Option<i64>,
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
    /// Unix-epoch (seconds) time the tile was downloaded/added/last refreshed, or `None`
    /// if unknown.
    pub fetched: Option<i64>,
    /// Unix-epoch (seconds) expiration time, or `None` for an entry that never expires.
    pub expires: Option<i64>,
    /// Upstream validator (e.g. an HTTP `ETag`), or `None`.
    pub etag: Option<&'a str>,
}

impl<'a> CacheEntryMeta<'a> {
    /// Create cache metadata with a `fetched` and an `expires` time (both Unix-epoch
    /// seconds) and an `etag`.
    ///
    /// For entries missing some of these, construct the struct directly (its fields are
    /// public) or use [`CacheEntryMeta::default`] for "unknown fetch time, never expires,
    /// no etag".
    #[must_use]
    pub fn new(fetched: i64, expires: i64, etag: &'a str) -> Self {
        Self {
            fetched: Some(fetched),
            expires: Some(expires),
            etag: Some(etag),
        }
    }
}

impl Mbtiles {
    /// Create the tile-cache schema (`metadata` and `tile_cache` tables plus the `tiles`
    /// view) if it does not already exist.
    ///
    /// Pass `strict = true` to create `STRICT` tables.
    pub async fn create_cache_schema<T>(&self, conn: &mut T, strict: bool) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        create_metadata_table(&mut *conn, strict).await?;
        create_cache_tables(&mut *conn, strict).await
    }

    /// Returns `true` if this file uses the tile-cache schema.
    pub async fn is_cache<T>(&self, conn: &mut T) -> MbtResult<bool>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        is_cache_tables_type(conn).await
    }

    /// Look up a cached tile by its XYZ coordinates.
    ///
    /// Returns the tile together with its `fetched`/`expires`/`etag` metadata, or `None`
    /// if there is no entry at the given coordinates. Expired entries are still returned
    /// (with their stored `expires`) so the caller can decide whether to serve stale,
    /// revalidate via `etag`, or refetch.
    pub async fn get_cached<T>(
        &self,
        conn: &mut T,
        z: u8,
        x: u32,
        y: u32,
    ) -> MbtResult<Option<CachedTile>>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let row = query(
            "SELECT tile_data, fetched, expires, etag
             FROM tile_cache
             WHERE zoom_level = ?1 AND tile_column = ?2 AND tile_row = ?3",
        )
        .bind(z)
        .bind(x)
        .bind(invert_y_value(z, y))
        .fetch_optional(conn)
        .await?;

        Ok(row.map(|row| CachedTile {
            data: row.get(0),
            fetched: row.get(1),
            expires: row.get(2),
            etag: row.get(3),
        }))
    }

    /// Insert or replace a cached tile, with its [`CacheEntryMeta`]
    /// (`fetched`/`expires`/`etag`).
    pub async fn set_cached(
        &self,
        conn: &mut SqliteConnection,
        z: u8,
        x: u32,
        y: u32,
        data: &[u8],
        meta: CacheEntryMeta<'_>,
    ) -> MbtResult<()> {
        query(
            "INSERT OR REPLACE INTO tile_cache
                 (zoom_level, tile_column, tile_row, fetched, expires, etag, tile_data)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(z)
        .bind(x)
        .bind(invert_y_value(z, y))
        .bind(meta.fetched)
        .bind(meta.expires)
        .bind(meta.etag)
        .bind(data)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Update only the `fetched`/`expires`/`etag` metadata of an existing cache entry,
    /// without touching the tile blob.
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
            "UPDATE tile_cache SET fetched = ?4, expires = ?5, etag = ?6
             WHERE zoom_level = ?1 AND tile_column = ?2 AND tile_row = ?3",
        )
        .bind(z)
        .bind(x)
        .bind(invert_y_value(z, y))
        .bind(meta.fetched)
        .bind(meta.expires)
        .bind(meta.etag)
        .execute(conn)
        .await?
        .rows_affected();
        Ok(updated > 0)
    }

    /// Delete all entries whose `expires` timestamp is strictly less than `now` (a Unix-epoch
    /// seconds value).
    ///
    /// Returns the number of `tile_cache` rows removed. Entries with `expires IS NULL` never
    /// expire and are left untouched. Freed pages are released back to the OS via
    /// `PRAGMA incremental_vacuum`, which only shrinks the file when it has
    /// `auto_vacuum` enabled (files created by [`crate::MbtilesCache::open`] do); for other
    /// files the pages are reused by later writes, and a one-off full `VACUUM` is needed
    /// to shrink them on disk.
    pub async fn purge_expired(&self, conn: &mut SqliteConnection, now: i64) -> MbtResult<u64> {
        let removed = query!(
            "DELETE FROM tile_cache WHERE expires IS NOT NULL AND expires < ?1",
            now
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        query!("PRAGMA incremental_vacuum")
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
        max_bytes: u64,
    ) -> MbtResult<u64> {
        /// How many `tile_cache` rows to evict between size re-measurements. Small enough
        /// to not massively overshoot the target, large enough to amortize the PRAGMAs.
        const EVICT_CHUNK: u32 = 64;
        let mut removed = 0;
        while db_live_size(&mut *conn).await? > max_bytes {
            let evicted = query!(
                "DELETE FROM tile_cache WHERE (zoom_level, tile_column, tile_row) IN
                     (SELECT zoom_level, tile_column, tile_row FROM tile_cache
                      ORDER BY expires IS NULL, expires LIMIT ?1)",
                EVICT_CHUNK
            )
            .execute(&mut *conn)
            .await?
            .rows_affected();

            if evicted == 0 {
                break; // the cache is empty; what remains is fixed schema overhead
            }

            query!("PRAGMA incremental_vacuum")
                .execute(&mut *conn)
                .await?;
            removed += evicted;
        }
        Ok(removed)
    }
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
    use crate::{CacheEntryMeta, Mbtiles};

    /// Open an in-memory cache file with the schema created.
    async fn cache() -> (Mbtiles, sqlx::SqliteConnection) {
        let mbt = Mbtiles::new(":memory:").unwrap();
        let mut conn = mbt.open().await.unwrap();
        mbt.create_cache_schema(&mut conn, false).await.unwrap();
        (mbt, conn)
    }

    /// Count the cache entries.
    async fn entry_count(conn: &mut sqlx::SqliteConnection) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM tile_cache")
            .fetch_one(conn)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn detected_as_cache() {
        let (mbt, mut conn) = cache().await;
        assert!(mbt.is_cache(&mut conn).await.unwrap());
        assert_eq!(
            mbt.detect_type(&mut conn).await.unwrap(),
            crate::MbtType::Cache
        );
    }

    #[tokio::test]
    async fn set_get_roundtrip() {
        let (mbt, mut conn) = cache().await;
        assert!(mbt.get_cached(&mut conn, 3, 1, 2).await.unwrap().is_none());

        mbt.set_cached(
            &mut conn,
            3,
            1,
            2,
            b"hello",
            CacheEntryMeta::new(42, 100, "etag-1"),
        )
        .await
        .unwrap();
        let got = mbt.get_cached(&mut conn, 3, 1, 2).await.unwrap().unwrap();
        assert_eq!(got.data, b"hello");
        assert_eq!(got.fetched, Some(42));
        assert_eq!(got.expires, Some(100));
        assert_eq!(got.etag.as_deref(), Some("etag-1"));

        // Overwrite the same coordinate with new data/metadata.
        mbt.set_cached(&mut conn, 3, 1, 2, b"world", CacheEntryMeta::default())
            .await
            .unwrap();
        let got = mbt.get_cached(&mut conn, 3, 1, 2).await.unwrap().unwrap();
        assert_eq!(got.data, b"world");
        assert_eq!(got.fetched, None);
        assert_eq!(got.expires, None);
        assert_eq!(got.etag, None);
        assert_eq!(entry_count(&mut conn).await, 1);
    }

    #[tokio::test]
    async fn readable_via_tiles_view() {
        let (mbt, mut conn) = cache().await;
        // z=1, y=0 (XYZ) => TMS tile_row = 1
        mbt.set_cached(&mut conn, 1, 0, 0, b"viewdata", CacheEntryMeta::default())
            .await
            .unwrap();
        let data = mbt.get_tile(&mut conn, 1, 0, 0).await.unwrap().unwrap();
        assert_eq!(data, b"viewdata");
    }

    #[tokio::test]
    async fn purge_expired_removes() {
        let (mbt, mut conn) = cache().await;
        let stale = CacheEntryMeta {
            expires: Some(50),
            ..Default::default()
        };
        mbt.set_cached(&mut conn, 0, 0, 0, b"stale", stale)
            .await
            .unwrap();
        let fresh = CacheEntryMeta {
            expires: Some(200),
            ..Default::default()
        };
        mbt.set_cached(&mut conn, 1, 0, 0, b"fresh", fresh)
            .await
            .unwrap();
        mbt.set_cached(&mut conn, 1, 1, 0, b"forever", CacheEntryMeta::default())
            .await
            .unwrap();

        let removed = mbt.purge_expired(&mut conn, 100).await.unwrap();
        assert_eq!(removed, 1);

        assert!(mbt.get_cached(&mut conn, 0, 0, 0).await.unwrap().is_none());
        assert!(mbt.get_cached(&mut conn, 1, 0, 0).await.unwrap().is_some());
        assert!(mbt.get_cached(&mut conn, 1, 1, 0).await.unwrap().is_some());
        assert_eq!(entry_count(&mut conn).await, 2);

        // Nothing left to expire: a second purge is a no-op.
        assert_eq!(mbt.purge_expired(&mut conn, 100).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn update_meta_without_rewriting_blob() {
        let (mbt, mut conn) = cache().await;
        mbt.set_cached(
            &mut conn,
            3,
            1,
            2,
            b"payload",
            CacheEntryMeta::new(42, 100, "etag-1"),
        )
        .await
        .unwrap();

        // No entry at these coordinates - the caller must fall back to set_cached.
        let missing = CacheEntryMeta::new(1, 1, "x");
        assert!(
            !mbt.update_cached_meta(&mut conn, 3, 0, 0, missing)
                .await
                .unwrap()
        );

        // Revalidation bumps the metadata in place; the blob stays untouched.
        let bumped = CacheEntryMeta::new(20, 500, "etag-2");
        assert!(
            mbt.update_cached_meta(&mut conn, 3, 1, 2, bumped)
                .await
                .unwrap()
        );
        let got = mbt.get_cached(&mut conn, 3, 1, 2).await.unwrap().unwrap();
        assert_eq!(got.data, b"payload");
        assert_eq!(got.fetched, Some(20));
        assert_eq!(got.expires, Some(500));
        assert_eq!(got.etag.as_deref(), Some("etag-2"));
        assert_eq!(entry_count(&mut conn).await, 1);
    }

    #[tokio::test]
    async fn empty_blob_caches_negative_response() {
        let (mbt, mut conn) = cache().await;
        assert!(mbt.get_cached(&mut conn, 5, 1, 1).await.unwrap().is_none());

        // Cached negative: Some(empty) with its own freshness metadata.
        mbt.set_cached(
            &mut conn,
            5,
            1,
            1,
            b"",
            CacheEntryMeta::new(5, 60, "miss-etag"),
        )
        .await
        .unwrap();
        let got = mbt.get_cached(&mut conn, 5, 1, 1).await.unwrap().unwrap();
        assert!(got.data.is_empty());
        assert_eq!(got.fetched, Some(5));
        assert_eq!(got.expires, Some(60));
        assert_eq!(got.etag.as_deref(), Some("miss-etag"));
    }

    #[tokio::test]
    async fn insert_tiles_bulk() {
        let (mbt, mut conn) = cache().await;
        let batch: Vec<(u8, u32, u32, Vec<u8>)> = vec![
            (1, 0, 0, b"same".to_vec()),
            (1, 1, 0, b"same".to_vec()),
            (1, 1, 1, b"other".to_vec()),
        ];
        mbt.insert_tiles(
            &mut conn,
            crate::MbtType::Cache,
            crate::CopyDuplicateMode::Override,
            &batch,
        )
        .await
        .unwrap();

        assert_eq!(entry_count(&mut conn).await, 3);
        let got = mbt.get_cached(&mut conn, 1, 1, 0).await.unwrap().unwrap();
        assert_eq!(got.data, b"same");
        assert_eq!(got.fetched, None, "bulk-inserted tiles have no fetch time");
        assert_eq!(got.expires, None, "bulk-inserted tiles never expire");
        assert_eq!(got.etag, None);
    }

    #[tokio::test]
    async fn purge_to_size_evicts_expiring_first() {
        let (mbt, mut conn) = cache().await;
        // 80 expiring + 20 never-expiring tiles, each a distinct 8 KiB blob (larger than
        // a page, so evictions free their overflow pages immediately).
        for i in 0..100u32 {
            let data = vec![u8::try_from(i % 251).unwrap(); 8192];
            let meta = if i < 80 {
                CacheEntryMeta {
                    expires: Some(i64::from(i)),
                    ..Default::default()
                }
            } else {
                CacheEntryMeta::default()
            };
            mbt.set_cached(&mut conn, 9, i, 0, &data, meta)
                .await
                .unwrap();
        }

        let initial = super::db_live_size(&mut conn).await.unwrap();
        let budget = initial - 300 * 1024;
        let removed = mbt.purge_cache_to_size(&mut conn, budget).await.unwrap();
        assert!(removed > 0);
        assert!(super::db_live_size(&mut conn).await.unwrap() <= budget);

        // Soonest-expiring entries went first; never-expiring ones survived.
        assert!(mbt.get_cached(&mut conn, 9, 0, 0).await.unwrap().is_none());
        assert!(mbt.get_cached(&mut conn, 9, 99, 0).await.unwrap().is_some());

        // Already under budget: a second call is a no-op.
        assert_eq!(mbt.purge_cache_to_size(&mut conn, budget).await.unwrap(), 0);
    }
}

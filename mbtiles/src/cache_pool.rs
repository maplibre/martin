//! Pooled, writable handle for using an `.mbtiles` file as a persistent tile cache.
//!
//! [`MbtilesCache`] is the cache counterpart of the read-only [`crate::MbtilesPool`]:
//! it opens the file read-write (creating it if missing), enables WAL journaling and a
//! busy timeout so concurrent readers and a single writer coexist, and exposes only the
//! operations that make sense for a cache - the read/write/purge API of the `cache`
//! module plus read-only tile serving. It deliberately has no general tile-writing API;
//! use the connection-level [`Mbtiles`] methods for that.
//!
//! See the `cache` module for the schema and single-connection API.

use std::path::Path;
use std::time::Duration;

use sqlx::sqlite::{SqliteAutoVacuum, SqliteConnectOptions, SqliteJournalMode};
use sqlx::{SqlitePool, query_scalar};

use crate::errors::MbtResult;
use crate::{CacheEntryMeta, CacheSchema, CachedTile, MbtError, Mbtiles, Metadata};

/// Connection pool for using an `.mbtiles` file as a **writable** tile cache.
///
/// Created with [`MbtilesCache::open`]. Cheap to [`Clone`] and safe to share across
/// tasks; every method acquires a connection from the pool.
///
/// # Examples
///
/// ```
/// use mbtiles::{CacheEntryMeta, MbtilesCache};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let cache = MbtilesCache::open("cache.mbtiles").await?;
///
/// // Store a downloaded tile with its fetch time and freshness metadata.
/// cache.set_cached(3, 1, 2, b"tile-bytes", CacheEntryMeta::new(1700000000, 1700003600, "etag-1")).await?;
///
/// // Later: read it back, expired entries included (freshness is the caller's call).
/// if let Some(tile) = cache.get_cached(3, 1, 2).await? {
///     println!("{} bytes, expires {:?}", tile.data.len(), tile.expires);
/// }
///
/// // After an HTTP 304 revalidation, bump the metadata without rewriting the blob.
/// cache.update_cached_meta(3, 1, 2, CacheEntryMeta::new(1700003600, 1700007200, "etag-1")).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct MbtilesCache {
    mbtiles: Mbtiles,
    pool: SqlitePool,
    schema: CacheSchema,
}

impl MbtilesCache {
    /// Open (creating if missing) an `.mbtiles` file as a writable tile cache, using the
    /// [`CacheSchema::Normalized`] layout (de-duplicated blobs - the best default for
    /// web-tile caches) when creating a new file.
    ///
    /// See [`MbtilesCache::open_with_schema`] for the details and errors.
    #[hotpath::measure]
    pub async fn open<P: AsRef<Path>>(filepath: P) -> MbtResult<Self> {
        Self::open_with_schema(filepath, CacheSchema::Normalized).await
    }

    /// Open (creating if missing) an `.mbtiles` file as a writable tile cache, creating
    /// a new file with the given [`CacheSchema`] layout.
    ///
    /// The connection pool is read-write with WAL journaling, a 5s busy timeout, and
    /// incremental `auto_vacuum` (effective for newly created files; pre-existing files
    /// keep their mode until a full `VACUUM`). The tile-cache schema is created if the
    /// database is empty; an existing cache file keeps its own layout, which
    /// [`MbtilesCache::schema`] reports.
    ///
    /// # Errors
    ///
    /// Returns [`MbtError::NotACacheFile`] if the file already exists with any other
    /// (non-cache) schema - e.g. a regular `MBTiles` tileset - to avoid silently mixing
    /// cache tables into it.
    #[hotpath::measure]
    pub async fn open_with_schema<P: AsRef<Path>>(
        filepath: P,
        schema: CacheSchema,
    ) -> MbtResult<Self> {
        let mbtiles = Mbtiles::new(filepath)?;
        let opt = SqliteConnectOptions::new()
            .filename(mbtiles.filepath())
            .create_if_missing(true)
            .auto_vacuum(SqliteAutoVacuum::Incremental)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePool::connect_with(opt).await?;
        let mut conn = pool.acquire().await?;
        // Only a new/empty database or an existing cache file is acceptable; anything
        // else (e.g. a regular tileset) must not silently gain cache tables.
        let objects: i64 = query_scalar("SELECT COUNT(*) FROM sqlite_master")
            .fetch_one(&mut *conn)
            .await?;
        let schema = if objects == 0 {
            mbtiles
                .create_cache_schema(&mut *conn, schema, false)
                .await?;
            schema
        } else {
            // An existing cache file keeps its layout; the requested one is ignored
            match mbtiles.cache_schema(&mut *conn).await? {
                Some(existing) => existing,
                None => return Err(MbtError::NotACacheFile(mbtiles.filepath().to_string())),
            }
        };
        drop(conn);
        Ok(Self {
            mbtiles,
            pool,
            schema,
        })
    }

    /// The [`CacheSchema`] layout of this cache file.
    #[must_use]
    pub fn schema(&self) -> CacheSchema {
        self.schema
    }

    /// Look up a cached tile and its `fetched`/`expires`/`etag` metadata.
    ///
    /// See [`Mbtiles::get_cached`] for the semantics (expired entries are still returned).
    #[hotpath::measure]
    pub async fn get_cached(&self, z: u8, x: u32, y: u32) -> MbtResult<Option<CachedTile>> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles
            .get_cached(&mut *conn, self.schema, z, x, y)
            .await
    }

    /// Insert or replace a cached tile with its [`CacheEntryMeta`] (`fetched`/`expires`/`etag`).
    ///
    /// See [`Mbtiles::set_cached`] for de-duplication and collision behavior.
    #[hotpath::measure]
    pub async fn set_cached(
        &self,
        z: u8,
        x: u32,
        y: u32,
        data: &[u8],
        meta: CacheEntryMeta<'_>,
    ) -> MbtResult<()> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles
            .set_cached(&mut conn, self.schema, z, x, y, data, meta)
            .await
    }

    /// Update only the `fetched`/`expires`/`etag` metadata of an existing entry (revalidation).
    ///
    /// Returns `false` if there is no entry at the given coordinates.
    /// See [`Mbtiles::update_cached_meta`].
    #[hotpath::measure]
    pub async fn update_cached_meta(
        &self,
        z: u8,
        x: u32,
        y: u32,
        meta: CacheEntryMeta<'_>,
    ) -> MbtResult<bool> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles
            .update_cached_meta(&mut *conn, z, x, y, meta)
            .await
    }

    /// Delete entries expired before `now` (Unix-epoch seconds) and any orphaned blobs.
    ///
    /// Returns the number of `tile_cache` rows removed. See [`Mbtiles::purge_expired`].
    #[hotpath::measure]
    pub async fn purge_expired(&self, now: i64) -> MbtResult<u64> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles
            .purge_expired(&mut conn, self.schema, now)
            .await
    }

    /// Evict entries (soonest-expiring first) until the live size is at most `max_bytes`.
    ///
    /// Returns the number of `tile_cache` rows removed. See [`Mbtiles::purge_cache_to_size`].
    #[hotpath::measure]
    pub async fn purge_cache_to_size(&self, max_bytes: u64) -> MbtResult<u64> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles
            .purge_cache_to_size(&mut conn, self.schema, max_bytes)
            .await
    }

    /// Read a tile through the spec-compatible `tiles` view, without cache metadata.
    ///
    /// See [`Mbtiles::get_tile`].
    #[hotpath::measure]
    pub async fn get_tile(&self, z: u8, x: u32, y: u32) -> MbtResult<Option<Vec<u8>>> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles.get_tile(&mut *conn, z, x, y).await
    }

    /// Read the `metadata` table contents.
    ///
    /// See [`Mbtiles::get_metadata`].
    #[hotpath::measure]
    pub async fn get_metadata(&self) -> MbtResult<Metadata> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles.get_metadata(&mut *conn).await
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::CacheEntryMeta;
    use crate::CacheSchema::{Flat, Normalized};

    #[rstest]
    #[tokio::test]
    async fn cache_roundtrip_and_persist(#[values(Flat, Normalized)] schema: CacheSchema) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.mbtiles");

        // Create a fresh cache file, write two tiles (one expiring, one permanent).
        {
            let cache = MbtilesCache::open_with_schema(&path, schema).await.unwrap();
            assert_eq!(cache.schema(), schema);
            cache
                .set_cached(2, 1, 1, b"tile-a", CacheEntryMeta::new(40, 50, "v1"))
                .await
                .unwrap();
            cache
                .set_cached(2, 1, 2, b"tile-b", CacheEntryMeta::default())
                .await
                .unwrap();
        }

        // Reopen the existing file: entries persist, the layout is detected (the default
        // `open` ignores its normalized preference), and it is still a readable tileset.
        let cache = MbtilesCache::open(&path).await.unwrap();
        assert_eq!(cache.schema(), schema);
        let a = cache.get_cached(2, 1, 1).await.unwrap().unwrap();
        assert_eq!(a.data, b"tile-a");
        assert_eq!(a.fetched, Some(40));
        assert_eq!(a.expires, Some(50));
        assert_eq!(a.etag.as_deref(), Some("v1"));
        assert_eq!(cache.get_tile(2, 1, 2).await.unwrap().unwrap(), b"tile-b");

        // Revalidate the expiring entry without rewriting its blob.
        assert!(
            cache
                .update_cached_meta(2, 1, 1, CacheEntryMeta::new(60, 75, "v1"))
                .await
                .unwrap()
        );
        let a = cache.get_cached(2, 1, 1).await.unwrap().unwrap();
        assert_eq!(a.fetched, Some(60));
        assert_eq!(a.expires, Some(75));

        // Purge the expired entry; the permanent one survives.
        assert_eq!(cache.purge_expired(100).await.unwrap(), 1);
        assert!(cache.get_cached(2, 1, 1).await.unwrap().is_none());
        assert!(cache.get_cached(2, 1, 2).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn new_file_gets_incremental_auto_vacuum() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("av.mbtiles");
        let cache = MbtilesCache::open(&path).await.unwrap();
        let mut conn = cache.pool.acquire().await.unwrap();
        let mode: i64 = query_scalar("PRAGMA auto_vacuum")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(mode, 2, "expected auto_vacuum = INCREMENTAL");
    }

    #[tokio::test]
    async fn refuses_non_cache_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flat.mbtiles");

        // Make a minimal flat tileset.
        let mbt = Mbtiles::new(&path).unwrap();
        let mut conn = mbt.open_or_new().await.unwrap();
        sqlx::query("CREATE TABLE metadata (name text NOT NULL PRIMARY KEY, value text)")
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE tiles (zoom_level integer NOT NULL, tile_column integer NOT NULL,
             tile_row integer NOT NULL, tile_data blob,
             PRIMARY KEY(zoom_level, tile_column, tile_row))",
        )
        .execute(&mut conn)
        .await
        .unwrap();
        drop(conn);

        let err = MbtilesCache::open(&path).await.unwrap_err();
        assert!(matches!(err, MbtError::NotACacheFile(_)), "got {err:?}");
    }
}

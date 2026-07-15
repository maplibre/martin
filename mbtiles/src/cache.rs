//! Use an `.mbtiles` (`SQLite`) file as a persistent, de-duplicating tile cache.
//!
//! This is a **non-standard** schema (not part of the `MBTiles` specification) built on
//! top of the same `SQLite` file format. It stores tiles together with cache metadata
//! (`expires` and `etag`) and de-duplicates identical tile blobs, and it is intended to
//! be embedded by other systems that need a simple on-disk tile cache.
//!
//! # Schema
//!
//! - `cache_data(tile_id INTEGER PRIMARY KEY, tile_data BLOB)` - the tile blobs.
//!   `tile_id` is the [xxh3-64](https://github.com/Cyan4973/xxHash) hash of `tile_data`,
//!   stored as an `INTEGER PRIMARY KEY` so it aliases the rowid (single B-tree, no
//!   secondary index). Identical blobs collapse to one row.
//! - `tile_cache(zoom_level, tile_column, tile_row, expires, etag, tile_id)` - a
//!   `WITHOUT ROWID` index table clustered on `(zoom_level, tile_column, tile_row)`,
//!   with a `tile_id` foreign key into `cache_data`.
//! - `tiles` view - a spec-compatible read view, so the file can still be opened by any
//!   standard `MBTiles` reader (the `expires`/`etag` columns are simply invisible to it).
//!
//! Coordinates use the XYZ (Slippy map) scheme on the API, matching the rest of the crate;
//! the TMS `tile_row` inversion is handled internally.
//!
//! See [`crate::MbtilesPool::open_cache`] for a pooled, writable entry point.

use sqlx::{Connection as _, SqliteConnection, SqliteExecutor, query};
use tracing::debug;
use xxhash_rust::xxh3::xxh3_64;

use crate::errors::MbtResult;
use crate::queries::create_metadata_table;
use crate::schemas::{create_cache_tables, is_cache_tables_type};
use crate::{MbtError, Mbtiles, invert_y_value};

/// Maximum number of linear probes when resolving an xxh3-64 collision in `set_cached`
/// before giving up with [`MbtError::CacheKeyExhausted`]. Collisions require billions of
/// distinct blobs to even begin, so this is only a safety valve against pathological input.
const MAX_KEY_PROBES: u32 = 1024;

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
/// numerically cast) to keep a lossless, round-trippable 64-bit key.
#[must_use]
fn content_key(data: &[u8]) -> i64 {
    i64::from_ne_bytes(xxh3_64(data).to_ne_bytes())
}

impl Mbtiles {
    /// Create the tile-cache schema (`metadata` and `tile_cache` + `cache_data` tables plus
    /// the `tiles` view) if it does not already exist.
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
    /// Returns the tile together with its `expires`/`etag` metadata, or `None` if there is
    /// no entry at the given coordinates. Expired entries are still returned (with their
    /// stored `expires`) so the caller can decide whether to serve stale, revalidate via
    /// `etag`, or refetch.
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
        let row = query!(
            "
SELECT d.tile_data, c.expires, c.etag
FROM tile_cache c
JOIN cache_data d ON d.tile_id = c.tile_id
WHERE c.zoom_level = ? AND c.tile_column = ? AND c.tile_row = ?",
            z,
            x,
            invert_y_value(z, y),
        )
        .fetch_optional(conn)
        .await?;

        Ok(row.map(|row| CachedTile {
            data: row.tile_data.unwrap_or_default(),
            expires: row.expires,
            etag: row.etag,
        }))
    }

    /// Insert or replace a cached tile, with its [`CacheEntryMeta`] (`expires`/`etag`).
    ///
    /// The tile blob is de-duplicated by content: identical blobs share a single
    /// `cache_data` row keyed on their xxh3-64 hash.
    ///
    /// On the astronomically-rare hash collision (a *different* blob already stored under the
    /// hash key), the key is resolved by **linear probing** - `key`, `key + 1`, … - until a
    /// free slot or a slot holding identical bytes is found. The resolved key is stored in the
    /// index row, so reads never probe and no existing entry is ever overwritten. If no slot
    /// is found within [`MAX_KEY_PROBES`], returns [`MbtError::CacheKeyExhausted`].
    pub async fn set_cached(
        &self,
        conn: &mut SqliteConnection,
        z: u8,
        x: u32,
        y: u32,
        data: &[u8],
        meta: CacheEntryMeta<'_>,
    ) -> MbtResult<()> {
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

        tx.commit().await?;
        Ok(())
    }

    /// Delete all entries whose `expires` timestamp is strictly less than `now` (a Unix-epoch
    /// seconds value), then remove any blobs that are no longer referenced.
    ///
    /// Returns the number of `tile_cache` rows removed. Entries with `expires IS NULL` never
    /// expire and are left untouched. This does not run `VACUUM`, so the file will not shrink
    /// on disk; run `VACUUM` separately if you need to reclaim space.
    pub async fn purge_expired(&self, conn: &mut SqliteConnection, now: i64) -> MbtResult<u64> {
        let mut tx = conn.begin().await?;
        let removed = query("DELETE FROM tile_cache WHERE expires IS NOT NULL AND expires < ?1")
            .bind(now)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        query("DELETE FROM cache_data WHERE tile_id NOT IN (SELECT tile_id FROM tile_cache)")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(removed)
    }
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

    #[tokio::test]
    async fn detected_as_cache() {
        let (mbt, mut conn) = cache().await;
        assert!(mbt.is_cache(&mut conn).await.unwrap());
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
            CacheEntryMeta::new(100, "etag-1"),
        )
        .await
        .unwrap();
        let got = mbt.get_cached(&mut conn, 3, 1, 2).await.unwrap().unwrap();
        assert_eq!(got.data, b"hello");
        assert_eq!(got.expires, Some(100));
        assert_eq!(got.etag.as_deref(), Some("etag-1"));

        // Overwrite the same coordinate with new data/metadata.
        mbt.set_cached(&mut conn, 3, 1, 2, b"world", CacheEntryMeta::default())
            .await
            .unwrap();
        let got = mbt.get_cached(&mut conn, 3, 1, 2).await.unwrap().unwrap();
        assert_eq!(got.data, b"world");
        assert_eq!(got.expires, None);
        assert_eq!(got.etag, None);
    }

    #[tokio::test]
    async fn blob_is_deduplicated() {
        let (mbt, mut conn) = cache().await;
        mbt.set_cached(&mut conn, 0, 0, 0, b"same", CacheEntryMeta::default())
            .await
            .unwrap();
        mbt.set_cached(&mut conn, 1, 0, 0, b"same", CacheEntryMeta::default())
            .await
            .unwrap();

        let blobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cache_data")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(blobs, 1, "identical blobs should share one cache_data row");
        let entries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tile_cache")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(entries, 2);
    }

    #[tokio::test]
    async fn collision_probes_next_slot_without_corrupting() {
        let (mbt, mut conn) = cache().await;
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
        mbt.set_cached(&mut conn, 4, 3, 2, victim, CacheEntryMeta::default())
            .await
            .unwrap();
        assert_eq!(
            mbt.get_cached(&mut conn, 4, 3, 2)
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
        mbt.set_cached(&mut conn, 5, 1, 1, victim, CacheEntryMeta::default())
            .await
            .unwrap();
        let blobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cache_data")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(blobs, 2, "squatter + victim only");
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
    async fn purge_expired_removes_and_gcs() {
        let (mbt, mut conn) = cache().await;
        mbt.set_cached(
            &mut conn,
            0,
            0,
            0,
            b"stale",
            CacheEntryMeta {
                expires: Some(50),
                etag: None,
            },
        )
        .await
        .unwrap();
        mbt.set_cached(
            &mut conn,
            1,
            0,
            0,
            b"fresh",
            CacheEntryMeta {
                expires: Some(200),
                etag: None,
            },
        )
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

        // The orphaned blob for the purged tile should be gone too.
        let blobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cache_data")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(blobs, 2);
    }
}

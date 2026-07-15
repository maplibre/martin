//! The `cache` schemas: non-standard tile-cache layouts used by [`crate::CachedTile`].
//!
//! Both layouts store tiles with expiration/etag metadata in a `tile_cache` table plus a
//! spec-compatible `tiles` view. [`CacheSchema::Flat`] keeps the blob inline, while
//! [`CacheSchema::Normalized`] de-duplicates blobs into a separate `cache_data` table.
//! This is not part of the `MBTiles` specification. See the `cache` module for the
//! read/write API.

use sqlx::{Row as _, SqliteExecutor, query};
use tracing::debug;

use crate::CacheSchema;
use crate::errors::MbtResult;
use crate::queries::create_schema;

/// Detect whether the database uses one of the tile-cache schemas, and which one.
///
/// Both layouts share a `tile_cache` table with `zoom_level`, `tile_column`, `tile_row`,
/// `fetched`, `expires`, and `etag` columns. The seventh column decides the layout: an
/// inline `tile_data` blob means [`CacheSchema::Flat`], while a `tile_id` integer joined
/// to a `cache_data(tile_id, tile_data)` blob table means [`CacheSchema::Normalized`].
///
/// This is a non-standard schema (not part of the `MBTiles` specification) used by
/// [`crate::CachedTile`] to store tiles with expiration/etag metadata. See the
/// `cache` module for the read/write API.
pub async fn cache_tables_schema<T>(conn: &mut T) -> MbtResult<Option<CacheSchema>>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    let row = query(
        "SELECT (
             -- Has a 'tile_cache' table
             SELECT COUNT(*) = 1
             FROM sqlite_master
             WHERE name = 'tile_cache' AND type = 'table'
             --
         ) AND (
             -- 'tile_cache' has exactly the six shared columns plus one layout column
             SELECT COUNT(*) = 7 FROM pragma_table_info('tile_cache')
             --
         ) AND (
             -- The six columns shared by both layouts are present with expected types
             SELECT COUNT(*) = 6
             FROM pragma_table_info('tile_cache')
             WHERE ((name = 'zoom_level' AND type LIKE '%INT%')
                 OR (name = 'tile_column' AND type LIKE '%INT%')
                 OR (name = 'tile_row' AND type LIKE '%INT%')
                 OR (name = 'fetched' AND type LIKE '%INT%')
                 OR (name = 'expires' AND type LIKE '%INT%')
                 OR (name = 'etag' AND type = 'TEXT'))
             --
         ) AS is_cache,
         (
             -- Flat layout: the tile blob is stored inline
             SELECT COUNT(*) = 1
             FROM pragma_table_info('tile_cache')
             WHERE name = 'tile_data' AND type = 'BLOB'
             --
         ) AS is_flat,
         (
             -- Normalized layout: an integer key into the 'cache_data' blob table
             (SELECT COUNT(*) = 1
              FROM pragma_table_info('tile_cache')
              WHERE name = 'tile_id' AND type LIKE '%INT%')
             AND
             (SELECT COUNT(*) = 2
              FROM pragma_table_info('cache_data')
              WHERE ((name = 'tile_id' AND type LIKE '%INT%')
                  OR (name = 'tile_data' AND type = 'BLOB')))
             --
         ) AS is_normalized;",
    )
    .fetch_one(&mut *conn)
    .await?;

    Ok(if row.get::<i64, _>("is_cache") != 1 {
        None
    } else if row.get::<i64, _>("is_flat") == 1 {
        Some(CacheSchema::Flat)
    } else if row.get::<i64, _>("is_normalized") == 1 {
        Some(CacheSchema::Normalized)
    } else {
        None
    })
}

/// Create the tile-cache tables and the standard `tiles` view (if they don't already exist).
///
/// - [`CacheSchema::Flat`]: `tile_cache(zoom_level, tile_column, tile_row, expires, etag,
///   tile_data)` clustered on `(zoom_level, tile_column, tile_row)`, blob stored inline.
/// - [`CacheSchema::Normalized`]: `tile_cache(zoom_level, tile_column, tile_row, expires,
///   etag, tile_id)` (`WITHOUT ROWID`) plus `cache_data(tile_id INTEGER PRIMARY KEY,
///   tile_data BLOB)`, where `tile_id` is the xxh3-64 content hash aliasing the rowid.
/// - Both create a spec-compatible `tiles` view so the file can still be read as a
///   normal `MBTiles` file.
///
/// See the `cache` module for the read/write API.
pub async fn create_cache_tables<T>(
    conn: &mut T,
    schema: CacheSchema,
    strict: bool,
) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!("Creating if needed {schema} cache tables and the tiles view");
    create_schema(conn, schema.init_sql(), strict).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::anonymous_mbtiles;
    use crate::{
        is_dedup_id_normalized_tables_type, is_flat_tables_type, is_flat_with_hash_tables_type,
        is_normalized_tables_type,
    };

    #[actix_rt::test]
    async fn create_and_detect_cache() {
        for schema in [CacheSchema::Flat, CacheSchema::Normalized] {
            let (_mbt, mut conn) = anonymous_mbtiles("").await;
            create_cache_tables(&mut conn, schema, false).await.unwrap();

            assert_eq!(
                cache_tables_schema(&mut conn).await.unwrap(),
                Some(schema),
                "{schema} cache should be detected as itself"
            );
            assert!(!is_flat_tables_type(&mut conn).await.unwrap());
            assert!(!is_flat_with_hash_tables_type(&mut conn).await.unwrap());
            assert!(!is_normalized_tables_type(&mut conn).await.unwrap());
            assert!(!is_dedup_id_normalized_tables_type(&mut conn).await.unwrap());
        }
    }
}

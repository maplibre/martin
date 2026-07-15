//! The `cache` schema: a non-standard tile-cache layout used by [`crate::CachedTile`].
//!
//! A single `tile_cache` table stores tiles with `fetched`/`expires`/`etag` metadata next
//! to the inline tile blob, plus a spec-compatible `tiles` view. This is not part of the
//! `MBTiles` specification. See the `cache` module for the read/write API.

use sqlx::{SqliteExecutor, query};
use tracing::debug;

use crate::errors::MbtResult;
use crate::queries::create_schema;

/// Check if the database uses the tile-cache schema: a `tile_cache` table with
/// `zoom_level`, `tile_column`, `tile_row`, `fetched`, `expires`, `etag`, and
/// `tile_data` columns.
///
/// This is a non-standard schema (not part of the `MBTiles` specification) used by
/// [`crate::CachedTile`] to store tiles with cache metadata. See the `cache` module for
/// the read/write API.
pub async fn is_cache_tables_type<T>(conn: &mut T) -> MbtResult<bool>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    let sql = query!(
        "SELECT (
             -- Has a 'tile_cache' table
             SELECT COUNT(*) = 1
             FROM sqlite_master
             WHERE name = 'tile_cache' AND type = 'table'
             --
         ) AND (
             -- 'tile_cache' table's columns and their types are as expected:
             -- 7 columns (zoom_level, tile_column, tile_row, fetched, expires, etag,
             -- tile_data). The order is not important
             SELECT COUNT(*) = 7
             FROM pragma_table_info('tile_cache')
             WHERE ((name = 'zoom_level' AND type LIKE '%INT%')
                 OR (name = 'tile_column' AND type LIKE '%INT%')
                 OR (name = 'tile_row' AND type LIKE '%INT%')
                 OR (name = 'fetched' AND type LIKE '%INT%')
                 OR (name = 'expires' AND type LIKE '%INT%')
                 OR (name = 'etag' AND type = 'TEXT')
                 OR (name = 'tile_data' AND type = 'BLOB'))
             --
         ) AS is_valid;"
    );

    Ok(sql.fetch_one(&mut *conn).await?.is_valid == 1)
}

/// Create the tile-cache table and the standard `tiles` view (if they don't already exist).
///
/// - `tile_cache(zoom_level, tile_column, tile_row, fetched, expires, etag, tile_data)`
///   clustered on `(zoom_level, tile_column, tile_row)`, blob stored inline.
/// - `tiles` view: a spec-compatible `(zoom_level, tile_column, tile_row, tile_data)`
///   view so the file can still be read as a normal `MBTiles` file.
///
/// See the `cache` module for the read/write API.
pub async fn create_cache_tables<T>(conn: &mut T, strict: bool) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!(
        "Creating if needed cache table and tiles view: tile_cache(z,x,y,fetched,expires,etag,data)"
    );
    create_schema(conn, include_str!("../../sql/init-cache.sql"), strict).await
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
        let (_mbt, mut conn) = anonymous_mbtiles("").await;
        create_cache_tables(&mut conn, false).await.unwrap();

        assert!(is_cache_tables_type(&mut conn).await.unwrap());
        assert!(!is_flat_tables_type(&mut conn).await.unwrap());
        assert!(!is_flat_with_hash_tables_type(&mut conn).await.unwrap());
        assert!(!is_normalized_tables_type(&mut conn).await.unwrap());
        assert!(!is_dedup_id_normalized_tables_type(&mut conn).await.unwrap());
    }
}

//! The `cache` schema: a non-standard tile-cache layout used by [`crate::CachedTile`].
//!
//! It stores tiles with expiration/etag metadata in a `tile_cache` index table joined to a
//! `cache_data` blob table, plus a spec-compatible `tiles` view. This is not part of the
//! `MBTiles` specification. See the `cache` module for the read/write API.

use sqlx::{SqliteExecutor, query};
use tracing::debug;

use crate::errors::MbtResult;
use crate::queries::create_schema;

/// Check if the database uses the tile-cache schema: a `tile_cache` index table
/// (`zoom_level`, `tile_column`, `tile_row`, `expires`, `etag`, `tile_id`) plus a
/// `cache_data` blob table (`tile_id`, `tile_data`).
///
/// This is a non-standard schema (not part of the `MBTiles` specification) used by
/// [`crate::CachedTile`] to store tiles with expiration/etag metadata. See the
/// `cache` module for the read/write API.
pub async fn is_cache_tables_type<T>(conn: &mut T) -> MbtResult<bool>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    let sql = query!(
        "SELECT (
             -- Has 'tile_cache' and 'cache_data' tables
             SELECT COUNT(*) = 2
             FROM sqlite_master
             WHERE (name = 'tile_cache' OR name = 'cache_data')
                 AND type = 'table'
             --
         ) AND (
             -- 'tile_cache' table's columns and their types are as expected:
             -- 6 columns (zoom_level, tile_column, tile_row, expires, etag, tile_id).
             -- The order is not important
             SELECT COUNT(*) = 6
             FROM pragma_table_info('tile_cache')
             WHERE ((name = 'zoom_level' AND type LIKE '%INT%')
                 OR (name = 'tile_column' AND type LIKE '%INT%')
                 OR (name = 'tile_row' AND type LIKE '%INT%')
                 OR (name = 'expires' AND type LIKE '%INT%')
                 OR (name = 'etag' AND type = 'TEXT')
                 OR (name = 'tile_id' AND type LIKE '%INT%'))
             --
         ) AND (
             -- 'cache_data' table's columns and their types are as expected:
             -- 2 columns (tile_id, tile_data).
             -- The order is not important
             SELECT COUNT(*) = 2
             FROM pragma_table_info('cache_data')
             WHERE ((name = 'tile_id' AND type LIKE '%INT%')
                 OR (name = 'tile_data' AND type = 'BLOB'))
             --
         ) AS is_valid;"
    );

    Ok(sql.fetch_one(&mut *conn).await?.is_valid == 1)
}

/// Create the tile-cache tables and the standard `tiles` view (if they don't already exist).
///
/// - `tile_cache(zoom_level, tile_column, tile_row, expires, etag, tile_id)`: a
///   `WITHOUT ROWID` table clustered on `(zoom_level, tile_column, tile_row)`.
/// - `cache_data(tile_id INTEGER PRIMARY KEY, tile_data BLOB)`: `tile_id` is an
///   `INTEGER PRIMARY KEY`, i.e. an alias for the rowid, so the xxh3-64 content hash
///   *is* the B-tree key with no secondary index.
/// - `tiles` view: a spec-compatible `(zoom_level, tile_column, tile_row, tile_data)`
///   view so the file can still be read as a normal `MBTiles` file.
///
/// See the `cache` module for the read/write API.
pub async fn create_cache_tables<T>(conn: &mut T, strict: bool) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!(
        "Creating if needed cache tables and tiles view: tile_cache(z,x,y,expires,etag,id) + cache_data(id,data)"
    );
    create_schema(conn, include_str!("../../sql/init-cache.sql"), strict).await
}

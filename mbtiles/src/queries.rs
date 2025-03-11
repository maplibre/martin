use martin_tile_utils::MAX_ZOOM;
use sqlite_compressions::rusqlite::Connection;
use sqlx::{query, Executor as _, Row, SqliteConnection, SqliteExecutor};
use tracing::debug;

use crate::bindiff::PatchType;
use crate::errors::MbtResult;
use crate::MbtError::InvalidZoomValue;
use crate::MbtType;

/// Returns true if the database is empty (no tables/indexes/...)
pub async fn is_empty_database<T>(conn: &mut T) -> MbtResult<bool>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    Ok(query!("SELECT 1 as has_rows FROM sqlite_schema LIMIT 1")
        .fetch_optional(&mut *conn)
        .await?
        .is_none())
}

pub async fn is_normalized_tables_type<T>(conn: &mut T) -> MbtResult<bool>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    let sql = query!(
        "SELECT (
             -- Has a 'map' table
             SELECT COUNT(*) = 1
             FROM sqlite_master
             WHERE name = 'map'
                 AND type = 'table'
             --
         ) AND (
             -- 'map' table's columns and their types are as expected:
             -- 4 columns (zoom_level, tile_column, tile_row, tile_id).
             -- The order is not important
             SELECT COUNT(*) = 4
             FROM pragma_table_info('map')
             WHERE ((name = 'zoom_level' AND type = 'INTEGER')
                 OR (name = 'tile_column' AND type = 'INTEGER')
                 OR (name = 'tile_row' AND type = 'INTEGER')
                 OR (name = 'tile_id' AND type = 'TEXT'))
             --
         ) AND (
             -- Has a 'images' table
             SELECT COUNT(*) = 1
             FROM sqlite_master
             WHERE name = 'images'
                 AND type = 'table'
             --
         ) AND (
             -- 'images' table's columns and their types are as expected:
             -- 2 columns (tile_id, tile_data).
             -- The order is not important
             SELECT COUNT(*) = 2
             FROM pragma_table_info('images')
             WHERE ((name = 'tile_id' AND type = 'TEXT')
                 OR (name = 'tile_data' AND type = 'BLOB'))
             --
         ) AS is_valid;"
    );

    Ok(sql.fetch_one(&mut *conn).await?.is_valid == 1)
}

/// Check if `MBTiles` has a table or a view named `tiles_with_hash` with needed fields
pub async fn has_tiles_with_hash<T>(conn: &mut T) -> MbtResult<bool>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    let sql = query!(
        "SELECT (
           -- 'tiles_with_hash' table or view columns and their types are as expected:
           -- 5 columns (zoom_level, tile_column, tile_row, tile_data, tile_hash).
           -- The order is not important
           SELECT COUNT(*) = 5
           FROM pragma_table_info('tiles_with_hash')
           WHERE ((name = 'zoom_level' AND type = 'INTEGER')
               OR (name = 'tile_column' AND type = 'INTEGER')
               OR (name = 'tile_row' AND type = 'INTEGER')
               OR (name = 'tile_data' AND type = 'BLOB')
               OR (name = 'tile_hash' AND type = 'TEXT'))
           --
       ) as is_valid;"
    );

    Ok(sql.fetch_one(&mut *conn).await?.is_valid == 1)
}

pub async fn is_flat_with_hash_tables_type<T>(conn: &mut T) -> MbtResult<bool>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    let sql = query!(
        "SELECT (
           -- Has a 'tiles_with_hash' table
           SELECT COUNT(*) = 1
           FROM sqlite_master
           WHERE name = 'tiles_with_hash'
               AND type = 'table'
           --
       ) as is_valid;"
    );

    let is_valid = sql.fetch_one(&mut *conn).await?.is_valid;

    Ok(is_valid == 1 && has_tiles_with_hash(&mut *conn).await?)
}

pub async fn is_flat_tables_type<T>(conn: &mut T) -> MbtResult<bool>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    let sql = query!(
        "SELECT (
             -- Has a 'tiles' table
             SELECT COUNT(*) = 1
             FROM sqlite_master
             WHERE name = 'tiles'
                 AND type = 'table'
             --
         ) AND (
             -- 'tiles' table's columns and their types are as expected:
             -- 4 columns (zoom_level, tile_column, tile_row, tile_data).
             -- The order is not important
             SELECT COUNT(*) = 4
             FROM pragma_table_info('tiles')
             WHERE ((name = 'zoom_level' AND type = 'INTEGER')
                 OR (name = 'tile_column' AND type = 'INTEGER')
                 OR (name = 'tile_row' AND type = 'INTEGER')
                 OR (name = 'tile_data' AND type = 'BLOB'))
             --
         ) as is_valid;"
    );

    Ok(sql.fetch_one(&mut *conn).await?.is_valid == 1)
}

pub async fn create_metadata_table<T>(conn: &mut T) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!("Creating metadata table if it doesn't already exist");
    conn.execute(
        "CREATE TABLE IF NOT EXISTS metadata (
             name text NOT NULL PRIMARY KEY,
             value text);",
    )
    .await?;

    Ok(())
}

pub async fn create_flat_tables<T>(conn: &mut T) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!("Creating if needed flat table: tiles(z,x,y,data)");
    conn.execute(
        "CREATE TABLE IF NOT EXISTS tiles (
             zoom_level integer NOT NULL,
             tile_column integer NOT NULL,
             tile_row integer NOT NULL,
             tile_data blob,
             PRIMARY KEY(zoom_level, tile_column, tile_row));",
    )
    .await?;

    Ok(())
}

pub async fn create_flat_with_hash_tables<T>(conn: &mut T) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!("Creating if needed flat-with-hash table: tiles_with_hash(z,x,y,data,hash)");
    conn.execute(
        "CREATE TABLE IF NOT EXISTS tiles_with_hash (
             zoom_level integer NOT NULL,
             tile_column integer NOT NULL,
             tile_row integer NOT NULL,
             tile_data blob,
             tile_hash text,
             PRIMARY KEY(zoom_level, tile_column, tile_row));",
    )
    .await?;

    debug!("Creating if needed tiles view for flat-with-hash");
    conn.execute(
        "CREATE VIEW IF NOT EXISTS tiles AS
             SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles_with_hash;",
    )
    .await?;

    Ok(())
}

#[must_use]
pub fn get_bsdiff_tbl_name(patch_type: PatchType) -> &'static str {
    match patch_type {
        PatchType::BinDiffRaw => "bsdiffraw",
        PatchType::BinDiffGz => "bsdiffrawgz",
    }
}

pub async fn create_bsdiffraw_tables<T>(conn: &mut T, patch_type: PatchType) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    let tbl = get_bsdiff_tbl_name(patch_type);
    debug!("Creating if needed bin-diff table: {tbl}(z,x,y,data,hash)");
    let sql = format!(
        "CREATE TABLE IF NOT EXISTS {tbl} (
             zoom_level integer NOT NULL,
             tile_column integer NOT NULL,
             tile_row integer NOT NULL,
             patch_data blob NOT NULL,
             tile_xxh3_64_hash integer NOT NULL,
             PRIMARY KEY(zoom_level, tile_column, tile_row));"
    );

    conn.execute(sql.as_str()).await?;
    Ok(())
}

/// Check if `MBTiles` has a table or a view named `bsdiffraw` or `bsdiffrawgz` with needed fields,
/// and return the corresponding patch type. If missing, return `PatchType::Whole`
pub async fn get_patch_type<T>(conn: &mut T) -> MbtResult<Option<PatchType>>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    for (tbl, pt) in [
        ("bsdiffraw", PatchType::BinDiffRaw),
        ("bsdiffrawgz", PatchType::BinDiffGz),
    ] {
        //  'bsdiffraw' or 'bsdiffrawgz' table or view columns and their types are as expected:
        //  5 columns (zoom_level, tile_column, tile_row, tile_data, tile_hash).
        //  The order is not important
        let sql = format!(
            "SELECT (
           SELECT COUNT(*) = 5
           FROM pragma_table_info('{tbl}')
           WHERE ((name = 'zoom_level' AND type = 'INTEGER')
               OR (name = 'tile_column' AND type = 'INTEGER')
               OR (name = 'tile_row' AND type = 'INTEGER')
               OR (name = 'patch_data' AND type = 'BLOB')
               OR (name = 'tile_xxh3_64_hash' AND type = 'INTEGER'))
           --
       ) as is_valid;"
        );

        if query(&sql)
            .fetch_one(&mut *conn)
            .await?
            .get::<Option<i32>, _>(0)
            .unwrap_or_default()
            == 1
        {
            return Ok(Some(pt));
        }
    }

    Ok(None)
}

pub async fn create_normalized_tables<T>(conn: &mut T) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!("Creating if needed normalized table: map(z,x,y,id)");
    conn.execute(
        "CREATE TABLE IF NOT EXISTS map (
             zoom_level integer NOT NULL,
             tile_column integer NOT NULL,
             tile_row integer NOT NULL,
             tile_id text,
             PRIMARY KEY(zoom_level, tile_column, tile_row));",
    )
    .await?;

    debug!("Creating if needed normalized table: images(id,data)");
    conn.execute(
        "CREATE TABLE IF NOT EXISTS images (
             tile_id text NOT NULL PRIMARY KEY,
             tile_data blob);",
    )
    .await?;

    debug!("Creating if needed tiles view for flat-with-hash");
    conn.execute(
        "CREATE VIEW IF NOT EXISTS tiles AS
             SELECT map.zoom_level AS zoom_level,
                    map.tile_column AS tile_column,
                    map.tile_row AS tile_row,
                    images.tile_data AS tile_data
             FROM map
             JOIN images ON images.tile_id = map.tile_id;",
    )
    .await?;

    Ok(())
}

pub async fn create_tiles_with_hash_view<T>(conn: &mut T) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!("Creating if needed tiles_with_hash view for normalized map+images structure");
    conn.execute(
        "CREATE VIEW IF NOT EXISTS tiles_with_hash AS
             SELECT
                 map.zoom_level AS zoom_level,
                 map.tile_column AS tile_column,
                 map.tile_row AS tile_row,
                 images.tile_data AS tile_data,
                 images.tile_id AS tile_hash
             FROM map
             JOIN images ON images.tile_id = map.tile_id",
    )
    .await?;

    Ok(())
}

pub async fn reset_db_settings<T>(conn: &mut T) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!("Resetting PRAGMA settings and vacuuming");
    query!("PRAGMA page_size = 512").execute(&mut *conn).await?;
    query!("PRAGMA encoding = 'UTF-8'")
        .execute(&mut *conn)
        .await?;
    query!("VACUUM").execute(&mut *conn).await?;
    Ok(())
}

pub async fn init_mbtiles_schema<T>(conn: &mut T, mbt_type: MbtType) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    reset_db_settings(conn).await?;
    create_metadata_table(&mut *conn).await?;
    match mbt_type {
        MbtType::Flat => create_flat_tables(&mut *conn).await,
        MbtType::FlatWithHash => create_flat_with_hash_tables(&mut *conn).await,
        MbtType::Normalized { hash_view } => {
            create_normalized_tables(&mut *conn).await?;
            if hash_view {
                create_tiles_with_hash_view(&mut *conn).await?;
            }
            Ok(())
        }
    }
}

/// Execute `DETACH DATABASE` command
pub async fn detach_db<T>(conn: &mut T, name: &str) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!("Detaching {name}");
    query(&format!("DETACH DATABASE {name}"))
        .execute(conn)
        .await?;
    Ok(())
}

fn validate_zoom(zoom: Option<i64>, zoom_name: &'static str) -> MbtResult<Option<u8>> {
    if let Some(zoom) = zoom {
        let z = u8::try_from(zoom).ok().filter(|v| *v <= MAX_ZOOM);
        if z.is_none() {
            Err(InvalidZoomValue(zoom_name, zoom.to_string()))
        } else {
            Ok(z)
        }
    } else {
        Ok(None)
    }
}

/// Compute min and max zoom levels from the `tiles` table
pub async fn compute_min_max_zoom<T>(conn: &mut T) -> MbtResult<Option<(u8, u8)>>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    let info = query!(
        "
SELECT min(zoom_level) AS min_zoom,
       max(zoom_level) AS max_zoom
FROM tiles;"
    )
    .fetch_one(conn)
    .await?;

    let min_zoom = validate_zoom(info.min_zoom, "zoom_level")?;
    let max_zoom = validate_zoom(info.max_zoom, "zoom_level")?;

    match (min_zoom, max_zoom) {
        (Some(min_zoom), Some(max_zoom)) => Ok(Some((min_zoom, max_zoom))),
        _ => Ok(None),
    }
}

pub async fn action_with_rusqlite(
    conn: &mut SqliteConnection,
    action: impl FnOnce(&Connection) -> MbtResult<()>,
) -> MbtResult<()> {
    // SAFETY: This must be scoped to make sure the handle is dropped before we continue using conn
    // Make sure not to execute any other queries while the handle is locked
    let mut handle_lock = conn.lock_handle().await?;
    let handle = handle_lock.as_raw_handle().as_ptr();

    // SAFETY: this is safe as long as handle_lock is valid. We will drop the lock.
    let rusqlite_conn = unsafe { Connection::from_handle(handle) }?;

    action(&rusqlite_conn)
}

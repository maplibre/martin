use sqlx::{query, SqliteExecutor};

use crate::errors::MbtResult;

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

    Ok(sql
        .fetch_one(&mut *conn)
        .await?
        .is_valid
        .unwrap_or_default()
        == 1)
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
       ) AND (
           -- 'tiles_with_hash' table's columns and their types are as expected:
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

    Ok(sql
        .fetch_one(&mut *conn)
        .await?
        .is_valid
        .unwrap_or_default()
        == 1)
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

    Ok(sql
        .fetch_one(&mut *conn)
        .await?
        .is_valid
        .unwrap_or_default()
        == 1)
}

pub async fn create_metadata_table<T>(conn: &mut T) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    query(
        "CREATE TABLE IF NOT EXISTS metadata (
             name text NOT NULL PRIMARY KEY,
             value text);",
    )
    .execute(&mut *conn)
    .await?;

    Ok(())
}

pub async fn create_flat_tables<T>(conn: &mut T) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    query(
        "CREATE TABLE IF NOT EXISTS tiles (
             zoom_level integer NOT NULL,
             tile_column integer NOT NULL,
             tile_row integer NOT NULL,
             tile_data blob,
             PRIMARY KEY(zoom_level, tile_column, tile_row));",
    )
    .execute(&mut *conn)
    .await?;

    Ok(())
}

pub async fn create_flat_with_hash_tables<T>(conn: &mut T) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    query(
        "CREATE TABLE IF NOT EXISTS tiles_with_hash (
             zoom_level integer NOT NULL,
             tile_column integer NOT NULL,
             tile_row integer NOT NULL,
             tile_data blob,
             tile_hash text,
             PRIMARY KEY(zoom_level, tile_column, tile_row));",
    )
    .execute(&mut *conn)
    .await?;

    query(
        "CREATE VIEW IF NOT EXISTS tiles AS
             SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles_with_hash;",
    )
    .execute(&mut *conn)
    .await?;

    Ok(())
}

pub async fn create_normalized_tables<T>(conn: &mut T) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    query(
        "CREATE TABLE IF NOT EXISTS map (
             zoom_level integer NOT NULL,
             tile_column integer NOT NULL,
             tile_row integer NOT NULL,
             tile_id text,
             PRIMARY KEY(zoom_level, tile_column, tile_row));",
    )
    .execute(&mut *conn)
    .await?;

    query(
        "CREATE TABLE IF NOT EXISTS images (
             tile_data blob,
             tile_id text NOT NULL PRIMARY KEY);",
    )
    .execute(&mut *conn)
    .await?;

    query(
        "CREATE VIEW IF NOT EXISTS tiles AS
             SELECT map.zoom_level AS zoom_level,
                    map.tile_column AS tile_column,
                    map.tile_row AS tile_row,
                    images.tile_data AS tile_data
             FROM map
             JOIN images ON images.tile_id = map.tile_id;",
    )
    .execute(&mut *conn)
    .await?;

    Ok(())
}

pub async fn create_tiles_with_hash_view<T>(conn: &mut T) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    query(
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
    .execute(&mut *conn)
    .await?;

    Ok(())
}

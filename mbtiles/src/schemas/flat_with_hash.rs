//! The `flat-with-hash` `MBTiles` schema: a `tiles_with_hash` table that adds a `tile_hash`
//! column to the flat schema, plus a `tiles` view for compatibility.

use sqlx::{AssertSqlSafe, Executor as _, SqliteExecutor, query};
use tracing::debug;

use crate::errors::MbtResult;

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
           WHERE ((name = 'zoom_level' AND type LIKE '%INT%')
               OR (name = 'tile_column' AND type LIKE '%INT%')
               OR (name = 'tile_row' AND type LIKE '%INT%')
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

pub async fn create_flat_with_hash_tables<T>(conn: &mut T, strict: bool) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!("Creating if needed flat-with-hash table: tiles_with_hash(z,x,y,data,hash)");
    let s = if strict { " STRICT" } else { "" };
    let sql = format!(
        "CREATE TABLE IF NOT EXISTS tiles_with_hash (
             zoom_level integer NOT NULL,
             tile_column integer NOT NULL,
             tile_row integer NOT NULL,
             tile_data blob,
             tile_hash text,
             PRIMARY KEY(zoom_level, tile_column, tile_row)){s};"
    );
    conn.execute(AssertSqlSafe(sql)).await?;

    debug!("Creating if needed tiles view for flat-with-hash");
    conn.execute(
        "CREATE VIEW IF NOT EXISTS tiles AS
             SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles_with_hash;",
    )
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::anonymous_mbtiles;
    use crate::{is_flat_tables_type, is_normalized_tables_type};

    #[actix_rt::test]
    async fn create_and_detect_flat_with_hash() {
        let (_mbt, mut conn) = anonymous_mbtiles("").await;
        create_flat_with_hash_tables(&mut conn, false)
            .await
            .unwrap();

        assert!(is_flat_with_hash_tables_type(&mut conn).await.unwrap());
        assert!(has_tiles_with_hash(&mut conn).await.unwrap());
        // 'tiles' is only a view here, so the flat (table) detection must not match
        assert!(!is_flat_tables_type(&mut conn).await.unwrap());
        assert!(!is_normalized_tables_type(&mut conn).await.unwrap());
    }
}

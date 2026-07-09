//! The `flat` `MBTiles` schema: a single `tiles(zoom_level, tile_column, tile_row, tile_data)` table.

use sqlx::{AssertSqlSafe, Executor as _, SqliteExecutor, query};
use tracing::debug;

use crate::errors::MbtResult;

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
             WHERE ((name = 'zoom_level' AND type LIKE '%INT%')
                 OR (name = 'tile_column' AND type LIKE '%INT%')
                 OR (name = 'tile_row' AND type LIKE '%INT%')
                 OR (name = 'tile_data' AND type = 'BLOB'))
             --
         ) as is_valid;"
    );

    Ok(sql.fetch_one(&mut *conn).await?.is_valid == 1)
}

pub async fn create_flat_tables<T>(conn: &mut T, strict: bool) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!("Creating if needed flat table: tiles(z,x,y,data)");
    let s = if strict { " STRICT" } else { "" };
    let sql = format!(
        "CREATE TABLE IF NOT EXISTS tiles (
             zoom_level integer NOT NULL,
             tile_column integer NOT NULL,
             tile_row integer NOT NULL,
             tile_data blob,
             PRIMARY KEY(zoom_level, tile_column, tile_row)){s};"
    );
    conn.execute(AssertSqlSafe(sql)).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::anonymous_mbtiles;
    use crate::schema_flat_with_hash::is_flat_with_hash_tables_type;
    use crate::schema_normalized::is_normalized_tables_type;

    #[actix_rt::test]
    async fn create_and_detect_flat() {
        let (_mbt, mut conn) = anonymous_mbtiles("").await;
        create_flat_tables(&mut conn, false).await.unwrap();

        assert!(is_flat_tables_type(&mut conn).await.unwrap());
        assert!(!is_flat_with_hash_tables_type(&mut conn).await.unwrap());
        assert!(!is_normalized_tables_type(&mut conn).await.unwrap());
    }
}

//! The `normalized` `MBTiles` schema: a `map(z,x,y,tile_id)` table joined to an
//! `images(tile_id, tile_data)` table, deduplicating identical tiles. An optional
//! `tiles_with_hash` view exposes the `tile_id` as a hash, and an alternative
//! `tiles_shallow` + `tiles_data` variant uses an integer `tile_data_id`.

use sqlx::{SqliteExecutor, query};
use tracing::debug;

use crate::errors::MbtResult;
use crate::queries::create_schema;

pub async fn is_normalized_tables_type<T>(conn: &mut T) -> MbtResult<bool>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    let sql = query!(
        "SELECT (
             -- Has 'map' and 'images' tables
             SELECT COUNT(*) = 2
             FROM sqlite_master
             WHERE (name = 'map' OR name = 'images')
                 AND type = 'table'
             --
         ) AND (
             -- 'map' table's columns and their types are as expected:
             -- 4 columns (zoom_level, tile_column, tile_row, tile_id).
             -- The order is not important
             SELECT COUNT(*) = 4
             FROM pragma_table_info('map')
             WHERE ((name = 'zoom_level' AND type LIKE '%INT%')
                 OR (name = 'tile_column' AND type LIKE '%INT%')
                 OR (name = 'tile_row' AND type LIKE '%INT%')
                 OR (name = 'tile_id' AND type = 'TEXT'))
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

/// Check if `MBTiles` has an alternative normalized schema with `tiles_shallow` + `tiles_data`
/// tables using integer `tile_data_id` instead of text `tile_id`.
pub async fn is_dedup_id_normalized_tables_type<T>(conn: &mut T) -> MbtResult<bool>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    let sql = query!(
        "SELECT (
             -- Has 'tiles_shallow' and 'tiles_data' tables
             SELECT COUNT(*) = 2
             FROM sqlite_master
             WHERE (name = 'tiles_shallow' OR name = 'tiles_data')
                 AND type = 'table'
             --
         ) AND (
             -- 'tiles_shallow' table's columns and their types are as expected:
             -- 4 columns (zoom_level, tile_column, tile_row, tile_data_id).
             -- The order is not important
             SELECT COUNT(*) = 4
             FROM pragma_table_info('tiles_shallow')
             WHERE ((name = 'zoom_level' AND type LIKE '%INT%')
                 OR (name = 'tile_column' AND type LIKE '%INT%')
                 OR (name = 'tile_row' AND type LIKE '%INT%')
                 OR (name = 'tile_data_id' AND type LIKE '%INT%'))
             --
         ) AND (
             -- 'tiles_data' table's columns and their types are as expected:
             -- 2 columns (tile_data_id, tile_data).
             -- The order is not important
             SELECT COUNT(*) = 2
             FROM pragma_table_info('tiles_data')
             WHERE ((name = 'tile_data_id' AND type LIKE '%INT%')
                 OR (name = 'tile_data' AND type = 'BLOB'))
             --
         ) AS is_valid;"
    );

    Ok(sql.fetch_one(&mut *conn).await?.is_valid == 1)
}

pub async fn create_normalized_tables<T>(conn: &mut T, strict: bool) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!("Creating if needed normalized tables and tiles view: map(z,x,y,id) + images(id,data)");
    create_schema(conn, include_str!("../../sql/init-normalized.sql"), strict).await
}

pub async fn create_dedup_id_normalized_tables<T>(conn: &mut T, strict: bool) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!(
        "Creating if needed dedup-id normalized tables and tiles view: tiles_shallow(z,x,y,id) + tiles_data(id,data)"
    );
    create_schema(
        conn,
        include_str!("../../sql/init-normalized-dedup-id.sql"),
        strict,
    )
    .await
}

pub async fn create_tiles_with_hash_view<T>(conn: &mut T) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!("Creating if needed tiles_with_hash view for normalized map+images structure");
    create_schema(
        conn,
        include_str!("../../sql/init-normalized-with-hash.sql"),
        false,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::anonymous_mbtiles;
    use crate::{
        CopyDuplicateMode, MbtType, NormalizedSchema, has_tiles_with_hash, init_mbtiles_schema,
        is_flat_tables_type,
    };

    #[actix_rt::test]
    async fn create_and_detect_normalized() {
        let (_mbt, mut conn) = anonymous_mbtiles("").await;
        create_normalized_tables(&mut conn, false).await.unwrap();

        assert!(is_normalized_tables_type(&mut conn).await.unwrap());
        assert!(!is_dedup_id_normalized_tables_type(&mut conn).await.unwrap());
        // 'tiles' is only a view here, so the flat (table) detection must not match
        assert!(!is_flat_tables_type(&mut conn).await.unwrap());

        // The hash view is optional and created separately
        assert!(!has_tiles_with_hash(&mut conn).await.unwrap());
        create_tiles_with_hash_view(&mut conn).await.unwrap();
        assert!(has_tiles_with_hash(&mut conn).await.unwrap());
    }

    #[actix_rt::test]
    async fn create_and_detect_dedup_id_normalized() {
        let (_mbt, mut conn) = anonymous_mbtiles("").await;
        create_dedup_id_normalized_tables(&mut conn, false)
            .await
            .unwrap();

        assert!(is_dedup_id_normalized_tables_type(&mut conn).await.unwrap());
        assert!(!is_normalized_tables_type(&mut conn).await.unwrap());
        assert!(!is_flat_tables_type(&mut conn).await.unwrap());
    }

    #[actix_rt::test]
    async fn insert_tiles_stores_each_distinct_tile_once_across_batches() {
        let (mbt, mut conn) = anonymous_mbtiles("").await;
        let dedup_id = MbtType::Normalized {
            hash_view: false,
            schema: NormalizedSchema::DedupId,
        };
        init_mbtiles_schema(&mut conn, dedup_id, false)
            .await
            .unwrap();

        let first: Vec<(u8, u32, u32, Vec<u8>)> = vec![
            (1, 0, 0, b"same".to_vec()),
            (1, 1, 0, b"same".to_vec()),
            (1, 1, 1, b"other".to_vec()),
        ];
        let second: Vec<(u8, u32, u32, Vec<u8>)> = vec![(1, 0, 1, b"same".to_vec())];
        for batch in [first, second] {
            mbt.insert_tiles(&mut conn, dedup_id, CopyDuplicateMode::Override, &batch)
                .await
                .unwrap();
        }

        let shallow: Vec<(i64, i64, i64, i64)> =
            sqlx::query_as("SELECT * FROM tiles_shallow ORDER BY 1, 2, 3")
                .fetch_all(&mut conn)
                .await
                .unwrap();
        assert_eq!(
            shallow,
            [(1, 0, 0, 1), (1, 0, 1, 1), (1, 1, 0, 2), (1, 1, 1, 1)]
        );
        assert_eq!(
            mbt.get_tile(&mut conn, 1, 1, 1).await.unwrap().unwrap(),
            b"other"
        );
    }

    #[actix_rt::test]
    async fn detect_dedup_id_normalized() {
        let script = include_str!("../../../tests/fixtures/mbtiles/normalized-dedup-id.sql");
        let (_mbt, mut conn) = anonymous_mbtiles(script).await;

        assert!(is_dedup_id_normalized_tables_type(&mut conn).await.unwrap());
    }
}

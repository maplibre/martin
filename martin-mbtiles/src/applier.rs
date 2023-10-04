use std::path::PathBuf;

use sqlx::query;

use crate::queries::detach_db;
use crate::MbtType::{Flat, FlatWithHash, Normalized};
use crate::{MbtResult, Mbtiles, AGG_TILES_HASH, AGG_TILES_HASH_IN_DIFF};

pub async fn apply_diff(src_file: PathBuf, diff_file: PathBuf) -> MbtResult<()> {
    let src_mbtiles = Mbtiles::new(src_file)?;
    let diff_mbtiles = Mbtiles::new(diff_file)?;
    let diff_type = diff_mbtiles.open_and_detect_type().await?;

    let mut conn = src_mbtiles.open().await?;
    diff_mbtiles.attach_to(&mut conn, "diffDb").await?;

    let src_type = src_mbtiles.detect_type(&mut conn).await?;
    let select_from = if src_type == Flat {
        "SELECT zoom_level, tile_column, tile_row, tile_data FROM diffDb.tiles"
    } else {
        match diff_type {
            Flat => {
                "SELECT zoom_level, tile_column, tile_row, tile_data, hex(md5(tile_data)) as hash
                 FROM diffDb.tiles"
            }
            FlatWithHash => {
                "SELECT zoom_level, tile_column, tile_row, tile_data, tile_hash AS hash
                 FROM diffDb.tiles_with_hash"
            }
            Normalized => {
                "SELECT zoom_level, tile_column, tile_row, tile_data, map.tile_id AS hash
                 FROM diffDb.map LEFT JOIN diffDb.images
                   ON diffDb.map.tile_id = diffDb.images.tile_id"
            }
        }
    }
    .to_string();

    let (main_table, insert_sql) = match src_type {
        Flat => ("tiles", vec![
            format!("INSERT OR REPLACE INTO tiles (zoom_level, tile_column, tile_row, tile_data) {select_from}")]),
        FlatWithHash => ("tiles_with_hash", vec![
            format!("INSERT OR REPLACE INTO tiles_with_hash (zoom_level, tile_column, tile_row, tile_data, tile_hash) {select_from}")]),
        Normalized => ("map", vec![
            format!("INSERT OR REPLACE INTO map (zoom_level, tile_column, tile_row, tile_id)
                     SELECT zoom_level, tile_column, tile_row, hash as tile_id
                     FROM ({select_from})"),
            format!("INSERT OR REPLACE INTO images (tile_id, tile_data)
                     SELECT hash as tile_id, tile_data
                     FROM ({select_from})"),
        ])
    };

    for statement in insert_sql {
        query(&format!("{statement} WHERE tile_data NOTNULL"))
            .execute(&mut conn)
            .await?;
    }

    query(&format!(
        "DELETE FROM {main_table}
             WHERE (zoom_level, tile_column, tile_row) IN (
                SELECT zoom_level, tile_column, tile_row FROM ({select_from} WHERE tile_data ISNULL)
             )"
    ))
    .execute(&mut conn)
    .await?;

    // Copy metadata from diffDb to the destination file, replacing existing values
    // Convert 'agg_tiles_hash_in_diff' into 'agg_tiles_hash'
    // Delete metadata entries if the value is NULL in diffDb
    query(&format!(
        "INSERT OR REPLACE INTO metadata (name, value)
         SELECT CASE WHEN name = '{AGG_TILES_HASH_IN_DIFF}' THEN '{AGG_TILES_HASH}' ELSE name END as name,
                value
         FROM diffDb.metadata
         WHERE name NOTNULL AND name != '{AGG_TILES_HASH}';"
    ))
    .execute(&mut conn)
    .await?;

    query(
        "DELETE FROM metadata
         WHERE name IN (SELECT name FROM diffDb.metadata WHERE value ISNULL);",
    )
    .execute(&mut conn)
    .await?;

    detach_db(&mut conn, "diffDb").await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MbtilesCopier;
    use sqlx::Executor as _;

    #[actix_rt::test]
    async fn apply_flat_diff_file() -> MbtResult<()> {
        // Copy the src file to an in-memory DB
        let src_file = PathBuf::from("../tests/fixtures/mbtiles/world_cities.mbtiles");
        let src = PathBuf::from("file:apply_flat_diff_file_mem_db?mode=memory&cache=shared");

        let mut src_conn = MbtilesCopier::new(src_file.clone(), src.clone())
            .run()
            .await?;

        // Apply diff to the src data in in-memory DB
        let diff_file = PathBuf::from("../tests/fixtures/mbtiles/world_cities_diff.mbtiles");
        apply_diff(src, diff_file).await?;

        // Verify the data is the same as the file the diff was generated from
        Mbtiles::new("../tests/fixtures/mbtiles/world_cities_modified.mbtiles")?
            .attach_to(&mut src_conn, "otherDb")
            .await?;

        assert!(src_conn
            .fetch_optional("SELECT * FROM tiles EXCEPT SELECT * FROM otherDb.tiles;")
            .await?
            .is_none());

        Ok(())
    }

    #[actix_rt::test]
    async fn apply_normalized_diff_file() -> MbtResult<()> {
        // Copy the src file to an in-memory DB
        let src_file = PathBuf::from("../tests/fixtures/mbtiles/geography-class-jpg.mbtiles");
        let src = PathBuf::from("file:apply_normalized_diff_file_mem_db?mode=memory&cache=shared");

        let mut src_conn = MbtilesCopier::new(src_file.clone(), src.clone())
            .run()
            .await?;

        // Apply diff to the src data in in-memory DB
        let diff_file = PathBuf::from("../tests/fixtures/mbtiles/geography-class-jpg-diff.mbtiles");
        apply_diff(src, diff_file).await?;

        // Verify the data is the same as the file the diff was generated from
        Mbtiles::new("../tests/fixtures/mbtiles/geography-class-jpg-modified.mbtiles")?
            .attach_to(&mut src_conn, "otherDb")
            .await?;

        assert!(src_conn
            .fetch_optional("SELECT * FROM tiles EXCEPT SELECT * FROM otherDb.tiles;")
            .await?
            .is_none());

        Ok(())
    }
}

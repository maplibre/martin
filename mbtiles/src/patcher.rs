use std::path::PathBuf;

use log::{debug, info};
use sqlx::query;

use crate::queries::detach_db;
use crate::MbtType::{Flat, FlatWithHash, Normalized};
use crate::{MbtResult, MbtType, Mbtiles, AGG_TILES_HASH, AGG_TILES_HASH_IN_DIFF};

pub async fn apply_patch(base_file: PathBuf, patch_file: PathBuf) -> MbtResult<()> {
    let base_mbt = Mbtiles::new(base_file)?;
    let patch_mbt = Mbtiles::new(patch_file)?;
    let patch_type = patch_mbt.open_and_detect_type().await?;

    let mut conn = base_mbt.open().await?;
    let base_type = base_mbt.detect_type(&mut conn).await?;
    patch_mbt.attach_to(&mut conn, "patchDb").await?;

    info!("Applying patch file {patch_mbt} ({patch_type}) to {base_mbt} ({base_type})");
    let select_from = get_select_from(base_type, patch_type);
    let (main_table, insert1, insert2) = get_insert_sql(base_type, select_from);

    query(&format!("{insert1} WHERE tile_data NOTNULL"))
        .execute(&mut conn)
        .await?;

    if let Some(insert2) = insert2 {
        query(&format!("{insert2} WHERE tile_data NOTNULL"))
            .execute(&mut conn)
            .await?;
    }

    query(&format!(
        "
    DELETE FROM {main_table}
    WHERE (zoom_level, tile_column, tile_row) IN (
        SELECT zoom_level, tile_column, tile_row FROM ({select_from} WHERE tile_data ISNULL)
    )"
    ))
    .execute(&mut conn)
    .await?;

    if base_type.is_normalized() {
        debug!("Removing unused tiles from the images table (normalized schema)");
        query("DELETE FROM images WHERE tile_id NOT IN (SELECT tile_id FROM map)")
            .execute(&mut conn)
            .await?;
    }

    // Copy metadata from patchDb to the destination file, replacing existing values
    // Convert 'agg_tiles_hash_in_patch' into 'agg_tiles_hash'
    // Delete metadata entries if the value is NULL in patchDb
    query(&format!(
        "
    INSERT OR REPLACE INTO metadata (name, value)
    SELECT IIF(name = '{AGG_TILES_HASH_IN_DIFF}', '{AGG_TILES_HASH}', name) as name,
           value
    FROM patchDb.metadata
    WHERE name NOTNULL AND name != '{AGG_TILES_HASH}';"
    ))
    .execute(&mut conn)
    .await?;

    query(
        "
    DELETE FROM metadata
    WHERE name IN (SELECT name FROM patchDb.metadata WHERE value ISNULL);",
    )
    .execute(&mut conn)
    .await?;

    detach_db(&mut conn, "patchDb").await
}

fn get_select_from(src_type: MbtType, patch_type: MbtType) -> &'static str {
    if src_type == Flat {
        "SELECT zoom_level, tile_column, tile_row, tile_data FROM patchDb.tiles"
    } else {
        match patch_type {
            Flat => {
                "
        SELECT zoom_level, tile_column, tile_row, tile_data, md5_hex(tile_data) as hash
        FROM patchDb.tiles"
            }
            FlatWithHash => {
                "
        SELECT zoom_level, tile_column, tile_row, tile_data, tile_hash AS hash
        FROM patchDb.tiles_with_hash"
            }
            Normalized { .. } => {
                "
        SELECT zoom_level, tile_column, tile_row, tile_data, map.tile_id AS hash
        FROM patchDb.map LEFT JOIN patchDb.images
          ON patchDb.map.tile_id = patchDb.images.tile_id"
            }
        }
    }
}

fn get_insert_sql(src_type: MbtType, select_from: &str) -> (&'static str, String, Option<String>) {
    match src_type {
        Flat => (
            "tiles",
            format!(
                "
    INSERT OR REPLACE INTO tiles (zoom_level, tile_column, tile_row, tile_data)
    {select_from}"
            ),
            None,
        ),
        FlatWithHash => (
            "tiles_with_hash",
            format!(
                "
    INSERT OR REPLACE INTO tiles_with_hash (zoom_level, tile_column, tile_row, tile_data, tile_hash)
    {select_from}"
            ),
            None,
        ),
        Normalized { .. } => (
            "map",
            format!(
                "
    INSERT OR REPLACE INTO map (zoom_level, tile_column, tile_row, tile_id)
    SELECT zoom_level, tile_column, tile_row, hash as tile_id
    FROM ({select_from})"
            ),
            Some(format!(
                "
    INSERT OR REPLACE INTO images (tile_id, tile_data)
    SELECT hash as tile_id, tile_data
    FROM ({select_from})"
            )),
        ),
    }
}

#[cfg(test)]
mod tests {
    use sqlx::Executor as _;

    use super::*;
    use crate::MbtilesCopier;

    #[actix_rt::test]
    async fn apply_flat_patch_file() -> MbtResult<()> {
        // Copy the src file to an in-memory DB
        let src_file = PathBuf::from("../tests/fixtures/mbtiles/world_cities.mbtiles");
        let src = PathBuf::from("file:apply_flat_diff_file_mem_db?mode=memory&cache=shared");

        let mut src_conn = MbtilesCopier {
            src_file: src_file.clone(),
            dst_file: src.clone(),
            ..Default::default()
        }
        .run()
        .await?;

        // Apply patch to the src data in in-memory DB
        let patch_file = PathBuf::from("../tests/fixtures/mbtiles/world_cities_diff.mbtiles");
        apply_patch(src, patch_file).await?;

        // Verify the data is the same as the file the patch was generated from
        Mbtiles::new("../tests/fixtures/mbtiles/world_cities_modified.mbtiles")?
            .attach_to(&mut src_conn, "testOtherDb")
            .await?;

        assert!(src_conn
            .fetch_optional("SELECT * FROM tiles EXCEPT SELECT * FROM testOtherDb.tiles;")
            .await?
            .is_none());

        Ok(())
    }

    #[actix_rt::test]
    async fn apply_normalized_patch_file() -> MbtResult<()> {
        // Copy the src file to an in-memory DB
        let src_file = PathBuf::from("../tests/fixtures/mbtiles/geography-class-jpg.mbtiles");
        let src = PathBuf::from("file:apply_normalized_diff_file_mem_db?mode=memory&cache=shared");

        let mut src_conn = MbtilesCopier {
            src_file: src_file.clone(),
            dst_file: src.clone(),
            ..Default::default()
        }
        .run()
        .await?;

        // Apply patch to the src data in in-memory DB
        let patch_file =
            PathBuf::from("../tests/fixtures/mbtiles/geography-class-jpg-diff.mbtiles");
        apply_patch(src, patch_file).await?;

        // Verify the data is the same as the file the patch was generated from
        Mbtiles::new("../tests/fixtures/mbtiles/geography-class-jpg-modified.mbtiles")?
            .attach_to(&mut src_conn, "testOtherDb")
            .await?;

        assert!(src_conn
            .fetch_optional("SELECT * FROM tiles EXCEPT SELECT * FROM testOtherDb.tiles;")
            .await?
            .is_none());

        Ok(())
    }
}

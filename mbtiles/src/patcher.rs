use std::path::PathBuf;

use log::{debug, info, warn};
use sqlx::{query, Connection as _};

use crate::queries::detach_db;
use crate::MbtType::{Flat, FlatWithHash, Normalized};
use crate::PatchType::Whole;
use crate::{
    MbtError, MbtResult, MbtType, Mbtiles, AGG_TILES_HASH, AGG_TILES_HASH_AFTER_APPLY,
    AGG_TILES_HASH_BEFORE_APPLY,
};

pub async fn apply_patch(base_file: PathBuf, patch_file: PathBuf, force: bool) -> MbtResult<()> {
    let base_mbt = Mbtiles::new(base_file)?;
    let patch_mbt = Mbtiles::new(patch_file)?;

    let mut conn = patch_mbt.open_readonly().await?;
    let patch_info = patch_mbt.examine_diff(&mut conn).await?;
    if patch_info.patch_type != Whole {
        return Err(MbtError::UnsupportedPatchType);
    }
    patch_mbt.validate_diff_info(&patch_info, force)?;
    let patch_type = patch_info.mbt_type;
    conn.close().await?;

    let mut conn = base_mbt.open().await?;
    let base_info = base_mbt.examine_diff(&mut conn).await?;
    let base_hash = base_mbt.get_agg_tiles_hash(&mut conn).await?;
    base_mbt.assert_hashes(&base_info, force)?;

    match (force, base_hash, patch_info.agg_tiles_hash_before_apply) {
        (false, Some(base_hash), Some(expected_hash)) if base_hash != expected_hash => {
            return Err(MbtError::AggHashMismatchWithDiff(
                patch_mbt.filepath().to_string(),
                expected_hash,
                base_mbt.filepath().to_string(),
                base_hash,
            ));
        }
        (true, Some(base_hash), Some(expected_hash)) if base_hash != expected_hash => {
            warn!("Aggregate tiles hash mismatch: Patch file expected {expected_hash} but found {base_hash} in {base_mbt} (force mode)");
        }
        _ => {}
    }

    info!(
        "Applying patch file {patch_mbt} ({patch_type}) to {base_mbt} ({base_type})",
        base_type = base_info.mbt_type
    );

    patch_mbt.attach_to(&mut conn, "patchDb").await?;
    let select_from = get_select_from(base_info.mbt_type, patch_type);
    let (main_table, insert1, insert2) = get_insert_sql(base_info.mbt_type, select_from);

    let sql = format!("{insert1} WHERE tile_data NOTNULL");
    query(&sql).execute(&mut conn).await?;

    if let Some(insert2) = insert2 {
        let sql = format!("{insert2} WHERE tile_data NOTNULL");
        query(&sql).execute(&mut conn).await?;
    }

    let sql = format!(
        "
    DELETE FROM {main_table}
    WHERE (zoom_level, tile_column, tile_row) IN (
        SELECT zoom_level, tile_column, tile_row FROM ({select_from} WHERE tile_data ISNULL)
    )"
    );
    query(&sql).execute(&mut conn).await?;

    if base_info.mbt_type.is_normalized() {
        debug!("Removing unused tiles from the images table (normalized schema)");
        let sql = "DELETE FROM images WHERE tile_id NOT IN (SELECT tile_id FROM map)";
        query(sql).execute(&mut conn).await?;
    }

    // Copy metadata from patchDb to the destination file, replacing existing values
    // Convert 'agg_tiles_hash_in_patch' into 'agg_tiles_hash'
    // Delete metadata entries if the value is NULL in patchDb
    let sql = format!(
        "
    INSERT OR REPLACE INTO metadata (name, value)
    SELECT IIF(name = '{AGG_TILES_HASH_AFTER_APPLY}', '{AGG_TILES_HASH}', name) as name,
           value
    FROM patchDb.metadata
    WHERE name NOTNULL AND name NOT IN ('{AGG_TILES_HASH}', '{AGG_TILES_HASH_BEFORE_APPLY}');"
    );
    query(&sql).execute(&mut conn).await?;

    let sql = "
    DELETE FROM metadata
    WHERE name IN (SELECT name FROM patchDb.metadata WHERE value ISNULL);";
    query(sql).execute(&mut conn).await?;

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
        apply_patch(src, patch_file, true).await?;

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
        apply_patch(src, patch_file, true).await?;

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

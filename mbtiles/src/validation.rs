use std::collections::HashSet;
use std::str::from_utf8;

use enum_display::EnumDisplay;
use log::{debug, info, warn};
use martin_tile_utils::{Format, MAX_ZOOM, TileInfo};
use serde::Serialize;
use serde_json::Value;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection, SqliteExecutor, query};
use tilejson::TileJSON;

use crate::MbtError::{
    AggHashMismatch, AggHashValueNotFound, FailedIntegrityCheck, IncorrectTileHash,
    InvalidTileIndex,
};
use crate::errors::{MbtError, MbtResult};
use crate::mbtiles::PatchFileInfo;
use crate::queries::{
    has_tiles_with_hash, is_flat_tables_type, is_flat_with_hash_tables_type,
    is_normalized_tables_type,
};
use crate::{Mbtiles, get_patch_type, invert_y_value};

/// Metadata key for the aggregate tiles hash value
pub const AGG_TILES_HASH: &str = "agg_tiles_hash";

/// Metadata key for a diff file, describing the eventual [`AGG_TILES_HASH`] value of the resulting tileset once the diff is applied
pub const AGG_TILES_HASH_AFTER_APPLY: &str = "agg_tiles_hash_after_apply";

/// Metadata key for a diff file, describing the expected [`AGG_TILES_HASH`] value of the tileset to which the diff will be applied.
pub const AGG_TILES_HASH_BEFORE_APPLY: &str = "agg_tiles_hash_before_apply";

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, EnumDisplay, Serialize)]
#[enum_display(case = "Kebab")]
pub enum MbtType {
    Flat,
    FlatWithHash,
    Normalized { hash_view: bool },
}

impl MbtType {
    #[must_use]
    pub fn is_normalized(self) -> bool {
        matches!(self, Self::Normalized { .. })
    }

    #[must_use]
    pub fn is_normalized_with_view(self) -> bool {
        matches!(self, Self::Normalized { hash_view: true })
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, EnumDisplay)]
#[enum_display(case = "Kebab")]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum IntegrityCheckType {
    #[default]
    Quick,
    Full,
    Off,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, EnumDisplay)]
#[enum_display(case = "Kebab")]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum AggHashType {
    /// Verify that the aggregate tiles hash value in the metadata table matches the computed value. Used by default.
    #[default]
    Verify,
    /// Update the aggregate tiles hash value in the metadata table
    Update,
    /// Do not check the aggregate tiles hash value
    Off,
}

impl Mbtiles {
    pub async fn open_and_validate(
        &self,
        check_type: IntegrityCheckType,
        agg_hash: AggHashType,
    ) -> MbtResult<String> {
        let mut conn = if agg_hash == AggHashType::Update {
            self.open().await?
        } else {
            self.open_readonly().await?
        };
        self.validate(&mut conn, check_type, agg_hash).await
    }

    pub async fn validate<T>(
        &self,
        conn: &mut T,
        check_type: IntegrityCheckType,
        agg_hash: AggHashType,
    ) -> MbtResult<String>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        self.check_integrity(&mut *conn, check_type).await?;
        self.check_tiles_type_validity(&mut *conn).await?;
        self.check_each_tile_hash(&mut *conn).await?;
        match agg_hash {
            AggHashType::Verify => self.check_agg_tiles_hashes(conn).await,
            AggHashType::Update => self.update_agg_tiles_hash(conn).await,
            AggHashType::Off => Ok(String::new()),
        }
    }

    /// Get the aggregate tiles hash value from the metadata table
    pub async fn get_agg_tiles_hash<T>(&self, conn: &mut T) -> MbtResult<Option<String>>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        self.get_metadata_value(&mut *conn, AGG_TILES_HASH).await
    }

    /// Detect tile format and verify that it is consistent across some tiles
    pub async fn detect_format<T>(&self, tilejson: &TileJSON, conn: &mut T) -> MbtResult<TileInfo>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let mut tile_info = None;
        let mut tested_zoom = -1_i64;

        // First, pick any random tile
        let query = query!(
            "SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles WHERE zoom_level >= 0 LIMIT 1"
        );
        let row = query.fetch_optional(&mut *conn).await?;
        if let Some(r) = row {
            tile_info = self.parse_tile(r.zoom_level, r.tile_column, r.tile_row, r.tile_data);
            tested_zoom = r.zoom_level.unwrap_or(-1);
        }

        // Afterward, iterate over tiles in all allowed zooms and check for consistency
        for z in tilejson.minzoom.unwrap_or(0)..=tilejson.maxzoom.unwrap_or(18) {
            if i64::from(z) == tested_zoom {
                continue;
            }
            let query = query! {"SELECT tile_column, tile_row, tile_data FROM tiles WHERE zoom_level = ? LIMIT 1", z};
            let row = query.fetch_optional(&mut *conn).await?;
            if let Some(r) = row {
                match (
                    tile_info,
                    self.parse_tile(Some(z.into()), r.tile_column, r.tile_row, r.tile_data),
                ) {
                    (_, None) => {}
                    (None, new) => tile_info = new,
                    (Some(old), Some(new)) if old == new => {}
                    (Some(old), Some(new)) => {
                        return Err(MbtError::InconsistentMetadata(old, new));
                    }
                }
            }
        }

        if let Some(Value::String(fmt)) = tilejson.other.get("format") {
            let file = self.filename();
            match (tile_info, Format::parse(fmt)) {
                (_, None) => {
                    warn!("Unknown format value in metadata: {fmt}");
                }
                (None, Some(fmt)) => {
                    if fmt.is_detectable() {
                        warn!(
                            "Metadata table sets detectable '{fmt}' tile format, but it could not be verified for file {file}"
                        );
                    } else {
                        info!("Using '{fmt}' tile format from metadata table in file {file}");
                    }
                    tile_info = Some(fmt.into());
                }
                (Some(info), Some(fmt)) if info.format == fmt => {
                    debug!(
                        "Detected tile format {info} matches metadata.format '{fmt}' in file {file}"
                    );
                }
                (Some(info), _) => {
                    warn!(
                        "Found inconsistency: metadata.format='{fmt}', but tiles were detected as {info:?} in file {file}. Tiles will be returned as {info:?}."
                    );
                }
            }
        }

        if let Some(info) = tile_info {
            if info.format != Format::Mvt && tilejson.vector_layers.is_some() {
                warn!(
                    "{} has vector_layers metadata but non-vector tiles",
                    self.filename()
                );
            }
            Ok(info)
        } else {
            Err(MbtError::NoTilesFound)
        }
    }

    fn parse_tile(
        &self,
        z: Option<i64>,
        x: Option<i64>,
        y: Option<i64>,
        tile: Option<Vec<u8>>,
    ) -> Option<TileInfo> {
        if let (Some(z), Some(x), Some(y), Some(tile)) = (z, x, y, tile) {
            let info = TileInfo::detect(&tile);
            if let Some(info) = info {
                debug!(
                    "Tile {z}/{x}/{} is detected as {info} in file {}",
                    {
                        if let (Ok(z), Ok(y)) = (u8::try_from(z), u32::try_from(y)) {
                            invert_y_value(z, y).to_string()
                        } else {
                            format!("{y} (invalid values, cannot invert Y)")
                        }
                    },
                    self.filename(),
                );
            }
            info
        } else {
            None
        }
    }

    pub async fn detect_type<T>(&self, conn: &mut T) -> MbtResult<MbtType>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        debug!("Detecting MBTiles type for {self}");
        let typ = if is_normalized_tables_type(&mut *conn).await? {
            MbtType::Normalized {
                hash_view: has_tiles_with_hash(&mut *conn).await?,
            }
        } else if is_flat_with_hash_tables_type(&mut *conn).await? {
            MbtType::FlatWithHash
        } else if is_flat_tables_type(&mut *conn).await? {
            MbtType::Flat
        } else {
            return Err(MbtError::InvalidDataFormat(self.filepath().to_string()));
        };

        self.check_for_uniqueness_constraint(&mut *conn, typ)
            .await?;

        Ok(typ)
    }

    async fn check_for_uniqueness_constraint<T>(
        &self,
        conn: &mut T,
        mbt_type: MbtType,
    ) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let table_name = match mbt_type {
            MbtType::Flat => "tiles",
            MbtType::FlatWithHash => "tiles_with_hash",
            MbtType::Normalized { .. } => "map",
        };

        let indexes = query("SELECT name FROM pragma_index_list(?) WHERE [unique] = 1")
            .bind(table_name)
            .fetch_all(&mut *conn)
            .await?;

        // Ensure there is some index on tiles that has a unique constraint on (zoom_level, tile_row, tile_column)
        for index in indexes {
            let mut unique_idx_cols = HashSet::new();
            let rows = query("SELECT DISTINCT name FROM pragma_index_info(?)")
                .bind(index.get::<String, _>("name"))
                .fetch_all(&mut *conn)
                .await?;

            for row in rows {
                unique_idx_cols.insert(row.get("name"));
            }

            if unique_idx_cols
                .symmetric_difference(&HashSet::from([
                    "zoom_level".to_string(),
                    "tile_column".to_string(),
                    "tile_row".to_string(),
                ]))
                .collect::<Vec<_>>()
                .is_empty()
            {
                return Ok(());
            }
        }

        Err(MbtError::NoUniquenessConstraint(
            self.filepath().to_string(),
        ))
    }

    /// Perform `SQLite` internal integrity check
    pub async fn check_integrity<T>(
        &self,
        conn: &mut T,
        integrity_check: IntegrityCheckType,
    ) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        if integrity_check == IntegrityCheckType::Off {
            info!("Skipping integrity check for {self}");
            return Ok(());
        }

        let sql = if integrity_check == IntegrityCheckType::Full {
            "PRAGMA integrity_check;"
        } else {
            "PRAGMA quick_check;"
        };

        let result: Vec<String> = query(sql)
            .map(|row: SqliteRow| row.get(0))
            .fetch_all(&mut *conn)
            .await?;

        if result.len() > 1
            || result.first().ok_or(FailedIntegrityCheck(
                self.filepath().to_string(),
                vec!["SQLite could not perform integrity check".to_string()],
            ))? != "ok"
        {
            return Err(FailedIntegrityCheck(self.filepath().to_string(), result));
        }

        info!("{integrity_check:?} integrity check passed for {self}");
        Ok(())
    }

    /// Check that the tiles table has the expected column, row, zoom, and data values
    pub async fn check_tiles_type_validity<T>(&self, conn: &mut T) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let sql = format!(
            "
SELECT zoom_level, tile_column, tile_row
FROM tiles
WHERE FALSE
   OR typeof(zoom_level) != 'integer'
   OR zoom_level < 0
   OR zoom_level > {MAX_ZOOM}
   OR typeof(tile_column) != 'integer'
   OR tile_column < 0
   OR tile_column >= (1 << zoom_level)
   OR typeof(tile_row) != 'integer'
   OR tile_row < 0
   OR tile_row >= (1 << zoom_level)
   OR (typeof(tile_data) != 'blob' AND typeof(tile_data) != 'null')
LIMIT 1;"
        );

        if let Some(row) = query(&sql).fetch_optional(&mut *conn).await? {
            let mut res: Vec<String> = Vec::with_capacity(3);
            for idx in (0..3).rev() {
                use sqlx::ValueRef as _;
                let raw = row.try_get_raw(idx)?;
                if raw.is_null() {
                    res.push("NULL".to_string());
                } else if let Ok(v) = row.try_get::<String, _>(idx) {
                    res.push(format!(r#""{v}" (TEXT)"#));
                } else if let Ok(v) = row.try_get::<Vec<u8>, _>(idx) {
                    res.push(format!(
                        r#""{}" (BLOB)"#,
                        from_utf8(&v).unwrap_or("<non-utf8-data>")
                    ));
                } else if let Ok(v) = row.try_get::<i32, _>(idx) {
                    res.push(format!("{v}"));
                } else if let Ok(v) = row.try_get::<f64, _>(idx) {
                    res.push(format!("{v} (REAL)"));
                } else {
                    res.push(format!("{:?}", raw.type_info()));
                }
            }

            return Err(InvalidTileIndex(
                self.filepath().to_string(),
                res.pop().unwrap(),
                res.pop().unwrap(),
                res.pop().unwrap(),
            ));
        }

        info!("All values in the `tiles` table/view are valid for {self}");
        Ok(())
    }

    pub async fn check_agg_tiles_hashes<T>(&self, conn: &mut T) -> MbtResult<String>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let Some(stored) = self.get_agg_tiles_hash(&mut *conn).await? else {
            return Err(AggHashValueNotFound(self.filepath().to_string()));
        };
        let computed = calc_agg_tiles_hash(&mut *conn).await?;
        if stored != computed {
            let file = self.filepath().to_string();
            return Err(AggHashMismatch(computed, stored, file));
        }

        info!("The agg_tiles_hashes={computed} has been verified for {self}");
        Ok(computed)
    }

    /// Compute new aggregate tiles hash and save it to the metadata table (if needed)
    pub async fn update_agg_tiles_hash<T>(&self, conn: &mut T) -> MbtResult<String>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let old_hash = self.get_agg_tiles_hash(&mut *conn).await?;
        let hash = calc_agg_tiles_hash(&mut *conn).await?;
        if old_hash.as_ref() == Some(&hash) {
            info!(
                "Metadata value agg_tiles_hash is already set to the correct hash `{hash}` in {self}"
            );
        } else {
            if let Some(old_hash) = old_hash {
                info!("Updating agg_tiles_hash from {old_hash} to {hash} in {self}");
            } else {
                info!("Adding a new metadata value agg_tiles_hash = {hash} in {self}");
            }
            self.set_metadata_value(&mut *conn, AGG_TILES_HASH, &hash)
                .await?;
        }
        Ok(hash)
    }

    pub async fn check_each_tile_hash<T>(&self, conn: &mut T) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        // Note that hex() always returns upper-case HEX values
        let sql = match self.detect_type(&mut *conn).await? {
            MbtType::Flat => {
                info!("Skipping per-tile hash validation because this is a flat MBTiles file");
                return Ok(());
            }
            MbtType::FlatWithHash => {
                "SELECT expected, computed FROM (
                    SELECT
                        upper(tile_hash) AS expected,
                        md5_hex(tile_data) AS computed
                    FROM tiles_with_hash
                ) AS t
                WHERE expected != computed
                LIMIT 1;"
            }
            MbtType::Normalized { .. } => {
                "SELECT expected, computed FROM (
                    SELECT
                        upper(tile_id) AS expected,
                        md5_hex(tile_data) AS computed
                    FROM images
                ) AS t
                WHERE expected != computed
                LIMIT 1;"
            }
        };

        query(sql)
            .fetch_optional(&mut *conn)
            .await?
            .map_or(Ok(()), |v| {
                Err(IncorrectTileHash(
                    self.filepath().to_string(),
                    v.get(0),
                    v.get(1),
                ))
            })?;

        info!("All tile hashes are valid for {self}");
        Ok(())
    }

    pub async fn examine_diff(&self, conn: &mut SqliteConnection) -> MbtResult<PatchFileInfo> {
        let info = PatchFileInfo {
            mbt_type: self.detect_type(&mut *conn).await?,
            agg_tiles_hash: self.get_agg_tiles_hash(&mut *conn).await?,
            agg_tiles_hash_before_apply: self
                .get_metadata_value(&mut *conn, AGG_TILES_HASH_BEFORE_APPLY)
                .await?,
            agg_tiles_hash_after_apply: self
                .get_metadata_value(&mut *conn, AGG_TILES_HASH_AFTER_APPLY)
                .await?,
            patch_type: get_patch_type(conn).await?,
        };

        Ok(info)
    }

    pub fn assert_hashes(&self, info: &PatchFileInfo, force: bool) -> MbtResult<()> {
        if info.agg_tiles_hash.is_none() {
            if !force {
                return Err(MbtError::CannotDiffFileWithoutHash(
                    self.filepath().to_string(),
                ));
            }
            warn!(
                "File {self} has no {AGG_TILES_HASH} metadata field, probably because it was created by an older version of the `mbtiles` tool.  Use this command to update the value:\nmbtiles validate --agg-hash update {self}"
            );
        } else if info.agg_tiles_hash_before_apply.is_some()
            || info.agg_tiles_hash_after_apply.is_some()
        {
            if !force {
                return Err(MbtError::DiffingDiffFile(self.filepath().to_string()));
            }
            warn!(
                "File {self} has {AGG_TILES_HASH_BEFORE_APPLY} or {AGG_TILES_HASH_AFTER_APPLY} metadata field, indicating it is a patch file which should not be diffed with another file."
            );
        }
        Ok(())
    }

    pub fn validate_diff_info(&self, info: &PatchFileInfo, force: bool) -> MbtResult<()> {
        match (
            &info.agg_tiles_hash_before_apply,
            &info.agg_tiles_hash_after_apply,
        ) {
            (Some(before), Some(after)) => {
                info!(
                    "The patch file {self} expects to be applied to a tileset with {AGG_TILES_HASH}={before}, and should result in hash {after} after applying",
                );
            }
            (None, Some(_)) => {
                if !force {
                    return Err(MbtError::PatchFileHasNoBeforeHash(
                        self.filepath().to_string(),
                    ));
                }
                warn!(
                    "The patch file {self} has no {AGG_TILES_HASH_BEFORE_APPLY} metadata field, probably because it was created by an older version of the `mbtiles` tool."
                );
            }
            _ => {
                if !force {
                    return Err(MbtError::PatchFileHasNoHashes(self.filepath().to_string()));
                }
                warn!(
                    "The patch file {self} has no {AGG_TILES_HASH_AFTER_APPLY} metadata field, probably because it was not properly created by the `mbtiles` tool."
                );
            }
        }
        Ok(())
    }
}

/// Compute the hash of the combined tiles in the mbtiles file tiles table/view.
/// This should work on all mbtiles files perf `MBTiles` specification.
pub async fn calc_agg_tiles_hash<T>(conn: &mut T) -> MbtResult<String>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!("Calculating agg_tiles_hash");
    let query = query(
        // The md5_concat func will return NULL if there are no rows in the tiles table.
        // For our use case, we will treat it as an empty string, and hash that.
        // `tile_data` values must be stored as a blob per MBTiles spec
        // `md5` functions will fail if the value is not text/blob/null
        //
        // Note that ORDER BY controls the output ordering, which is important for the hash value,
        // and we must use ORDER BY as a parameter to the aggregate function itself (available since SQLite 3.44.0)
        // See https://sqlite.org/forum/forumpost/228bb96e12a746ce
        "
SELECT coalesce(
           md5_concat_hex(
               cast(zoom_level AS text),
               cast(tile_column AS text),
               cast(tile_row AS text),
               tile_data
               ORDER BY zoom_level, tile_column, tile_row),
           md5_hex(''))
FROM tiles;
",
    );
    Ok(query.fetch_one(conn).await?.get::<String, _>(0))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::mbtiles::tests::open;

    #[actix_rt::test]
    async fn detect_type() -> MbtResult<()> {
        let (mut conn, mbt) = open("../tests/fixtures/mbtiles/world_cities.mbtiles").await?;
        let res = mbt.detect_type(&mut conn).await?;
        assert_eq!(res, MbtType::Flat);

        let (mut conn, mbt) = open("../tests/fixtures/mbtiles/zoomed_world_cities.mbtiles").await?;
        let res = mbt.detect_type(&mut conn).await?;
        assert_eq!(res, MbtType::FlatWithHash);

        let (mut conn, mbt) = open("../tests/fixtures/mbtiles/geography-class-jpg.mbtiles").await?;
        let res = mbt.detect_type(&mut conn).await?;
        assert_eq!(res, MbtType::Normalized { hash_view: false });

        let (mut conn, mbt) = open(":memory:").await?;
        let res = mbt.detect_type(&mut conn).await;
        assert!(matches!(res, Err(MbtError::InvalidDataFormat(_))));

        Ok(())
    }

    #[actix_rt::test]
    async fn validate_valid_file() -> MbtResult<()> {
        let (mut conn, mbt) = open("../tests/fixtures/mbtiles/zoomed_world_cities.mbtiles").await?;
        mbt.check_integrity(&mut conn, IntegrityCheckType::Quick)
            .await?;
        Ok(())
    }

    #[actix_rt::test]
    async fn validate_invalid_file() -> MbtResult<()> {
        let (mut conn, mbt) =
            open("../tests/fixtures/files/invalid_zoomed_world_cities.mbtiles").await?;
        let result = mbt.check_agg_tiles_hashes(&mut conn).await;
        assert!(matches!(result, Err(AggHashMismatch(..))));
        Ok(())
    }
}

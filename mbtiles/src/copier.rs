use std::fmt::Write as _;
use std::path::PathBuf;

use enum_display::EnumDisplay;
use itertools::Itertools as _;
use log::{debug, info, trace, warn};
use martin_tile_utils::{MAX_ZOOM, bbox_to_xyz};
use serde::{Deserialize, Serialize};
use sqlite_hashes::rusqlite::Connection;
use sqlx::{Connection as _, Executor as _, Row, SqliteConnection, query};
use tilejson::Bounds;

use crate::AggHashType::Verify;
use crate::IntegrityCheckType::Quick;
use crate::MbtType::{Flat, FlatWithHash, Normalized};
use crate::PatchType::BinDiffRaw;
use crate::bindiff::PatchType::BinDiffGz;
use crate::bindiff::{BinDiffDiffer, BinDiffPatcher, BinDiffer as _, PatchType};
use crate::errors::MbtResult;
use crate::mbtiles::PatchFileInfo;
use crate::queries::{
    create_tiles_with_hash_view, detach_db, init_mbtiles_schema, is_empty_database,
};
use crate::{
    AGG_TILES_HASH, AGG_TILES_HASH_AFTER_APPLY, AGG_TILES_HASH_BEFORE_APPLY, AggHashType, CopyType,
    MbtError, MbtType, MbtTypeCli, Mbtiles, action_with_rusqlite, get_bsdiff_tbl_name,
    invert_y_value, reset_db_settings,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumDisplay)]
#[enum_display(case = "Kebab")]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum CopyDuplicateMode {
    Override,
    Ignore,
    Abort,
}

impl CopyDuplicateMode {
    #[must_use]
    pub fn to_sql(self) -> &'static str {
        match self {
            CopyDuplicateMode::Override => "OR REPLACE",
            CopyDuplicateMode::Ignore => "OR IGNORE",
            CopyDuplicateMode::Abort => "OR ABORT",
        }
    }
}

#[derive(Clone, Default, PartialEq, Debug)]
pub struct MbtilesCopier {
    /// `MBTiles` file to read from
    pub src_file: PathBuf,
    /// `MBTiles` file to write to
    pub dst_file: PathBuf,
    /// Limit what gets copied
    pub copy: CopyType,
    /// Output format of the destination file, ignored if the file exists. If not specified, defaults to the type of source
    pub dst_type_cli: Option<MbtTypeCli>,
    /// Destination type with options
    pub dst_type: Option<MbtType>,
    /// Allow copying to existing files, and indicate what to do if a tile with the same Z/X/Y already exists
    pub on_duplicate: Option<CopyDuplicateMode>,
    /// Minimum zoom level to copy
    pub min_zoom: Option<u8>,
    /// Maximum zoom level to copy
    pub max_zoom: Option<u8>,
    /// List of zoom levels to copy
    pub zoom_levels: Vec<u8>,
    /// Bounding box to copy, in the format `min_lon,min_lat,max_lon,max_lat`. Can be used multiple times.
    pub bbox: Vec<Bounds>,
    /// Compare source file with this file, and only copy non-identical tiles to destination. Also specifies the type of patch to generate.
    pub diff_with_file: Option<(PathBuf, Option<PatchType>)>,
    /// Apply a patch file while copying src to dst.
    pub apply_patch: Option<PathBuf>,
    /// Skip generating a global hash for mbtiles validation. By default, `mbtiles` will compute `agg_tiles_hash` metadata value.
    pub skip_agg_tiles_hash: bool,
    /// Ignore some warnings and continue with the copying operation
    pub force: bool,
    /// Perform `agg_hash` validation on the original and destination files.
    pub validate: bool,
}

#[derive(Clone, Debug)]
struct MbtileCopierInt {
    src_mbt: Mbtiles,
    dst_mbt: Mbtiles,
    options: MbtilesCopier,
}

impl MbtilesCopier {
    pub async fn run(self) -> MbtResult<SqliteConnection> {
        MbtileCopierInt::new(self)?.run().await
    }

    pub(crate) fn dst_type(&self) -> Option<MbtType> {
        self.dst_type.or_else(|| {
            self.dst_type_cli.map(|t| match t {
                MbtTypeCli::Flat => Flat,
                MbtTypeCli::FlatWithHash => FlatWithHash,
                MbtTypeCli::Normalized => Normalized { hash_view: true },
            })
        })
    }
}

impl MbtileCopierInt {
    pub fn new(options: MbtilesCopier) -> MbtResult<Self> {
        if options.apply_patch.is_some() && options.diff_with_file.is_some() {
            return Err(MbtError::CannotApplyPatchAndDiff);
        }
        // We may want to resolve the files to absolute paths here, but will need to avoid various non-file cases
        if options.src_file == options.dst_file {
            return Err(MbtError::SameSourceAndDestination(options.src_file));
        }
        if let Some((diff_file, _)) = &options.diff_with_file {
            if options.src_file == *diff_file || options.dst_file == *diff_file {
                return Err(MbtError::SameDiffAndSourceOrDestination(options.src_file));
            }
        }
        if let Some(patch_file) = &options.apply_patch {
            if options.src_file == *patch_file || options.dst_file == *patch_file {
                return Err(MbtError::SameDiffAndSourceOrDestination(options.src_file));
            }
        }

        Ok(MbtileCopierInt {
            src_mbt: Mbtiles::new(&options.src_file)?,
            dst_mbt: Mbtiles::new(&options.dst_file)?,
            options,
        })
    }

    pub async fn run(self) -> MbtResult<SqliteConnection> {
        if let Some((diff_file, patch_type)) = &self.options.diff_with_file {
            let mbt = Mbtiles::new(diff_file)?;
            let patch_type = *patch_type;
            self.run_with_diff(mbt, patch_type).await
        } else if let Some(patch_file) = &self.options.apply_patch {
            let mbt = Mbtiles::new(patch_file)?;
            self.run_with_patch(mbt).await
        } else {
            self.run_simple().await
        }
    }

    async fn run_simple(self) -> MbtResult<SqliteConnection> {
        let mut conn = self.src_mbt.open_readonly().await?;
        let src_type = self.src_mbt.detect_type(&mut conn).await?;
        conn.close().await?;

        conn = self.dst_mbt.open_or_new().await?;
        let is_empty_db = is_empty_database(&mut conn).await?;

        let on_duplicate = if let Some(on_duplicate) = self.options.on_duplicate {
            on_duplicate
        } else if is_empty_db {
            CopyDuplicateMode::Override
        } else {
            return Err(MbtError::DestinationFileExists(self.options.dst_file));
        };

        self.src_mbt.attach_to(&mut conn, "sourceDb").await?;

        let dst_type = if is_empty_db {
            self.options.dst_type().unwrap_or(src_type)
        } else {
            self.validate_dst_type(self.dst_mbt.detect_type(&mut conn).await?)?
        };

        info!(
            "Copying {src_mbt} ({src_type}) {what}to a {is_new} file {dst_mbt} ({dst_type})",
            src_mbt = self.src_mbt,
            what = self.copy_text(),
            is_new = if is_empty_db { "new" } else { "existing" },
            dst_mbt = self.dst_mbt,
        );

        if is_empty_db {
            self.init_schema(&mut conn, src_type, dst_type).await?;
        }

        self.copy_with_rusqlite(
            &mut conn,
            on_duplicate,
            dst_type,
            get_select_from(src_type, dst_type),
        )
        .await?;

        if self.options.copy.copy_tiles() && !self.options.skip_agg_tiles_hash {
            self.dst_mbt.update_agg_tiles_hash(&mut conn).await?;
        }

        detach_db(&mut conn, "sourceDb").await?;

        Ok(conn)
    }

    /// Compare two files, and write their difference to the diff file
    async fn run_with_diff(
        self,
        dif_mbt: Mbtiles,
        patch_type: Option<PatchType>,
    ) -> MbtResult<SqliteConnection> {
        let mut dif_conn = dif_mbt.open_readonly().await?;
        let dif_info = dif_mbt.examine_diff(&mut dif_conn).await?;
        dif_mbt.assert_hashes(&dif_info, self.options.force)?;
        dif_conn.close().await?;

        let src_info = self.validate_src_file().await?;

        let mut conn = self.dst_mbt.open_or_new().await?;
        if !is_empty_database(&mut conn).await? {
            return Err(MbtError::NonEmptyTargetFile(self.options.dst_file));
        }

        self.src_mbt.attach_to(&mut conn, "sourceDb").await?;
        dif_mbt.attach_to(&mut conn, "diffDb").await?;

        let dst_type = self.options.dst_type().unwrap_or(src_info.mbt_type);
        if patch_type.is_some() && matches!(dst_type, Normalized { .. }) {
            return Err(MbtError::BinDiffRequiresFlatWithHash(dst_type));
        }

        info!(
            "Comparing {src_mbt} ({src_type}) and {dif_path} ({dif_type}) {what}into a new file {dst_path} ({dst_type}){patch}",
            src_mbt = self.src_mbt,
            src_type = src_info.mbt_type,
            dif_path = dif_mbt.filepath(),
            dif_type = dif_info.mbt_type,
            what = self.copy_text(),
            dst_path = self.dst_mbt.filepath(),
            patch = patch_type_str(patch_type),
        );

        self.init_schema(&mut conn, src_info.mbt_type, dst_type)
            .await?;
        self.copy_with_rusqlite(
            &mut conn,
            CopyDuplicateMode::Override,
            dst_type,
            &get_select_from_with_diff(dif_info.mbt_type, dst_type, patch_type),
        )
        .await?;

        // Bindiff copying uses separate threads to read and write data, so we need
        // to open a separate connection to source+diff files to avoid locking issues
        detach_db(&mut conn, "diffDb").await?;
        detach_db(&mut conn, "sourceDb").await?;

        if let Some(patch_type) = patch_type {
            BinDiffDiffer::new(self.src_mbt.clone(), dif_mbt, dif_info.mbt_type, patch_type)
                .run(&mut conn, self.get_where_clause("srcTiles."))
                .await?;
        }

        if let Some(hash) = src_info.agg_tiles_hash {
            self.dst_mbt
                .set_metadata_value(&mut conn, AGG_TILES_HASH_BEFORE_APPLY, &hash)
                .await?;
        }
        if let Some(hash) = dif_info.agg_tiles_hash {
            self.dst_mbt
                .set_metadata_value(&mut conn, AGG_TILES_HASH_AFTER_APPLY, &hash)
                .await?;
        }

        // TODO: perhaps disable all except --copy all when using with diffs, or else is not making much sense
        if self.options.copy.copy_tiles() && !self.options.skip_agg_tiles_hash {
            self.dst_mbt.update_agg_tiles_hash(&mut conn).await?;
        }

        self.validate(&self.dst_mbt, &mut conn).await?;

        Ok(conn)
    }

    /// Apply a patch file to the source file and write the result to the destination file
    async fn run_with_patch(self, dif_mbt: Mbtiles) -> MbtResult<SqliteConnection> {
        let mut dif_conn = dif_mbt.open_readonly().await?;
        let dif_info = dif_mbt.examine_diff(&mut dif_conn).await?;
        self.validate(&dif_mbt, &mut dif_conn).await?;
        dif_mbt.validate_diff_info(&dif_info, self.options.force)?;
        dif_conn.close().await?;

        let src_type = self.validate_src_file().await?.mbt_type;
        let dst_type = self.options.dst_type().unwrap_or(src_type);
        if dif_info.patch_type.is_some() && matches!(dst_type, Normalized { .. }) {
            return Err(MbtError::BinDiffRequiresFlatWithHash(dst_type));
        }

        let mut conn = self.dst_mbt.open_or_new().await?;
        if !is_empty_database(&mut conn).await? {
            return Err(MbtError::NonEmptyTargetFile(self.options.dst_file));
        }

        self.src_mbt.attach_to(&mut conn, "sourceDb").await?;
        dif_mbt.attach_to(&mut conn, "diffDb").await?;

        info!(
            "Applying patch from {dif_path} ({dif_type}) to {src_mbt} ({src_type}) {what}into a new file {dst_path} ({dst_type}){patch}",
            dif_path = dif_mbt.filepath(),
            dif_type = dif_info.mbt_type,
            src_mbt = self.src_mbt,
            what = self.copy_text(),
            dst_path = self.dst_mbt.filepath(),
            patch = patch_type_str(dif_info.patch_type),
        );

        self.init_schema(&mut conn, src_type, dst_type).await?;
        self.copy_with_rusqlite(
            &mut conn,
            CopyDuplicateMode::Override,
            dst_type,
            &get_select_from_apply_patch(src_type, &dif_info, dst_type),
        )
        .await?;

        detach_db(&mut conn, "diffDb").await?;
        detach_db(&mut conn, "sourceDb").await?;

        if let Some(patch_type) = dif_info.patch_type {
            BinDiffPatcher::new(self.src_mbt.clone(), dif_mbt.clone(), dst_type, patch_type)
                .run(&mut conn, self.get_where_clause("srcTiles."))
                .await?;
        }

        // TODO: perhaps disable all except --copy all when using with diffs, or else is not making much sense
        if self.options.copy.copy_tiles() && !self.options.skip_agg_tiles_hash {
            self.dst_mbt.update_agg_tiles_hash(&mut conn).await?;
            if matches!(dif_info.patch_type, Some(BinDiffGz)) {
                info!(
                    "Skipping {AGG_TILES_HASH_AFTER_APPLY} validation because re-gzip-ing could produce different tile data. Each bindiff-ed tile was still verified with a hash value"
                );
            } else {
                let new_hash = self.dst_mbt.get_agg_tiles_hash(&mut conn).await?;
                match (dif_info.agg_tiles_hash_after_apply, new_hash) {
                    (Some(expected), Some(actual)) if expected != actual => {
                        let err = MbtError::AggHashMismatchAfterApply(
                            dif_mbt.filepath().to_string(),
                            expected,
                            self.dst_mbt.filepath().to_string(),
                            actual,
                        );
                        if !self.options.force {
                            return Err(err);
                        }
                        warn!("{err}");
                    }
                    _ => {}
                }
            }
        }

        let hash_type =
            if matches!(dif_info.patch_type, Some(BinDiffGz)) || self.options.skip_agg_tiles_hash {
                AggHashType::Off
            } else {
                Verify
            };

        if self.options.validate {
            self.dst_mbt.validate(&mut conn, Quick, hash_type).await?;
        }

        Ok(conn)
    }

    /// Validate the integrity of the mbtiles file if requested
    /// 
    /// See [`Mbtiles::validate`] for the validations performed.
    async fn validate(&self, mbt: &Mbtiles, conn: &mut SqliteConnection) -> MbtResult<()> {
        if self.options.validate {
            mbt.validate(conn, Quick, Verify).await?;
        }
        Ok(())
    }

    async fn validate_src_file(&self) -> MbtResult<PatchFileInfo> {
        let mut src_conn = self.src_mbt.open_readonly().await?;
        let src_info = self.src_mbt.examine_diff(&mut src_conn).await?;
        self.validate(&self.src_mbt, &mut src_conn).await?;
        self.src_mbt.assert_hashes(&src_info, self.options.force)?;
        src_conn.close().await?;

        Ok(src_info)
    }

    fn copy_text(&self) -> &str {
        match self.options.copy {
            CopyType::All => "",
            CopyType::Tiles => "tiles data ",
            CopyType::Metadata => "metadata ",
        }
    }

    async fn copy_with_rusqlite(
        &self,
        conn: &mut SqliteConnection,
        on_duplicate: CopyDuplicateMode,
        dst_type: MbtType,
        select_from: &str,
    ) -> Result<(), MbtError> {
        if self.options.copy.copy_tiles() {
            action_with_rusqlite(conn, |c| {
                self.copy_tiles(c, dst_type, on_duplicate, select_from)
            })
            .await?;
        } else {
            debug!("Skipping copying tiles");
        }

        if self.options.copy.copy_metadata() {
            action_with_rusqlite(conn, |c| self.copy_metadata(c, on_duplicate)).await
        } else {
            debug!("Skipping copying metadata");
            Ok(())
        }
    }

    fn copy_metadata(
        &self,
        rusqlite_conn: &Connection,
        on_duplicate: CopyDuplicateMode,
    ) -> Result<(), MbtError> {
        let on_dupl = on_duplicate.to_sql();
        let sql;

        // Insert all rows from diffDb.metadata if they do not exist or are different in sourceDb.metadata.
        // Also insert all names from sourceDb.metadata that do not exist in diffDb.metadata, with their value set to NULL.
        // Skip agg_tiles_hash because that requires special handling
        if self.options.diff_with_file.is_some() {
            // Include agg_tiles_hash value even if it is the same because we will still need it when applying the diff
            sql = format!(
                "
    INSERT {on_dupl} INTO metadata (name, value)
        SELECT name, value
        FROM (
            SELECT COALESCE(difMD.name, srcMD.name) as name
                 , difMD.value as value
            FROM sourceDb.metadata AS srcMD FULL JOIN diffDb.metadata AS difMD
                 ON srcMD.name = difMD.name
            WHERE srcMD.value != difMD.value OR srcMD.value ISNULL OR difMD.value ISNULL
        ) joinedMD
        WHERE name NOT IN ('{AGG_TILES_HASH}', '{AGG_TILES_HASH_BEFORE_APPLY}', '{AGG_TILES_HASH_AFTER_APPLY}')"
            );
            debug!("Copying metadata, taking into account diff file with {sql}");
        } else if self.options.apply_patch.is_some() {
            sql = format!(
                "
    INSERT {on_dupl} INTO metadata (name, value)
        SELECT name, value
        FROM (
            SELECT COALESCE(srcMD.name, difMD.name) as name
                 , COALESCE(difMD.value, srcMD.value) as value
            FROM sourceDb.metadata AS srcMD FULL JOIN diffDb.metadata AS difMD
                 ON srcMD.name = difMD.name
            WHERE difMD.name ISNULL OR difMD.value NOTNULL
        ) joinedMD
        WHERE name NOT IN ('{AGG_TILES_HASH}', '{AGG_TILES_HASH_BEFORE_APPLY}', '{AGG_TILES_HASH_AFTER_APPLY}')"
            );
            debug!("Copying metadata, and applying the diff file with {sql}");
        } else {
            sql = format!(
                "
    INSERT {on_dupl} INTO metadata SELECT name, value FROM sourceDb.metadata"
            );
            debug!("Copying metadata with {sql}");
        }
        rusqlite_conn.execute(&sql, [])?;
        Ok(())
    }

    fn copy_tiles(
        &self,
        rusqlite_conn: &Connection,
        dst_type: MbtType,
        on_duplicate: CopyDuplicateMode,
        select_from: &str,
    ) -> Result<(), MbtError> {
        let on_dupl = on_duplicate.to_sql();
        let where_clause = self.get_where_clause("");
        let sql_cond = Self::get_on_duplicate_sql_cond(on_duplicate, dst_type);

        let sql = match dst_type {
            Flat => {
                format!(
                    "
    INSERT {on_dupl} INTO tiles
           (zoom_level, tile_column, tile_row, tile_data)
    {select_from} {where_clause} {sql_cond}"
                )
            }
            FlatWithHash => {
                format!(
                    "
    INSERT {on_dupl} INTO tiles_with_hash
           (zoom_level, tile_column, tile_row, tile_data, tile_hash)
    {select_from} {where_clause} {sql_cond}"
                )
            }
            Normalized { .. } => {
                let sql = format!(
                    "
    INSERT OR IGNORE INTO images
           (tile_id, tile_data)
    SELECT tile_hash as tile_id, tile_data
    FROM ({select_from} {where_clause})"
                );
                debug!("Copying to {dst_type} with {sql}");
                rusqlite_conn.execute(&sql, [])?;

                format!(
                    "
    INSERT {on_dupl} INTO map
           (zoom_level, tile_column, tile_row, tile_id)
    SELECT zoom_level, tile_column, tile_row, tile_hash as tile_id
    FROM ({select_from} {where_clause} {sql_cond})"
                )
            }
        };

        debug!("Copying to {dst_type} with {sql}");
        rusqlite_conn.execute(&sql, [])?;

        Ok(())
    }

    /// Check if the detected destination file type matches the one given by the options
    fn validate_dst_type(&self, dst_type: MbtType) -> MbtResult<MbtType> {
        if let Some(cli) = self.options.dst_type() {
            match (cli, dst_type) {
                (Flat, Flat)
                | (FlatWithHash, FlatWithHash)
                | (Normalized { .. }, Normalized { .. }) => {}
                (cli, dst) => {
                    return Err(MbtError::MismatchedTargetType(
                        self.options.dst_file.clone(),
                        dst,
                        cli,
                    ));
                }
            }
        }
        Ok(dst_type)
    }

    async fn init_schema(
        &self,
        conn: &mut SqliteConnection,
        src: MbtType,
        dst: MbtType,
    ) -> MbtResult<()> {
        if src == dst {
            reset_db_settings(conn).await?;
            debug!("Copying DB schema verbatim");
            // DB objects must be created in a specific order: tables, views, triggers, indexes.
            let sql_objects = conn
                .fetch_all(
                    "SELECT sql, tbl_name, type
                     FROM sourceDb.sqlite_schema
                     WHERE tbl_name IN ('metadata', 'tiles', 'map', 'images', 'tiles_with_hash')
                       AND type     IN ('table', 'view', 'trigger', 'index')
                     ORDER BY CASE
                         WHEN type = 'table' THEN 1
                         WHEN type = 'view' THEN 2
                         WHEN type = 'trigger' THEN 3
                         WHEN type = 'index' THEN 4
                         ELSE 5 END;",
                )
                .await?;

            for row in sql_objects {
                debug!(
                    "Creating {typ} {tbl_name}...",
                    typ = row.get::<&str, _>(2),
                    tbl_name = row.get::<&str, _>(1),
                );
                query(row.get(0)).execute(&mut *conn).await?;
            }
            if dst.is_normalized() {
                // Some normalized mbtiles files might not have this view, so even if src == dst, it might not exist
                create_tiles_with_hash_view(&mut *conn).await?;
            }
        } else {
            init_mbtiles_schema(&mut *conn, dst).await?;
        }

        Ok(())
    }

    /// Returns WHERE condition SQL depending on the override and destination type
    fn get_on_duplicate_sql_cond(on_duplicate: CopyDuplicateMode, dst_type: MbtType) -> String {
        match on_duplicate {
            CopyDuplicateMode::Ignore | CopyDuplicateMode::Override => String::new(),
            CopyDuplicateMode::Abort => {
                let (main_table, tile_identifier) = match dst_type {
                    Flat => ("tiles", "tile_data"),
                    FlatWithHash => ("tiles_with_hash", "tile_data"),
                    Normalized { .. } => ("map", "tile_id"),
                };

                format!(
                    "AND NOT EXISTS (
                             SELECT 1
                             FROM {main_table}
                             WHERE
                                 {main_table}.zoom_level = sourceDb.{main_table}.zoom_level
                                 AND {main_table}.tile_column = sourceDb.{main_table}.tile_column
                                 AND {main_table}.tile_row = sourceDb.{main_table}.tile_row
                                 AND {main_table}.{tile_identifier} != sourceDb.{main_table}.{tile_identifier}
                         )"
                )
            }
        }
    }

    /// Format SQL WHERE clause and return it along with the query arguments.
    /// Note that there is no risk of SQL injection here, as the arguments are integers.
    fn get_where_clause(&self, prefix: &str) -> String {
        let mut sql = if !&self.options.zoom_levels.is_empty() {
            let zooms = self.options.zoom_levels.iter().join(",");
            format!(" AND {prefix}zoom_level IN ({zooms})")
        } else if let Some(min_zoom) = self.options.min_zoom {
            if let Some(max_zoom) = self.options.max_zoom {
                format!(" AND {prefix}zoom_level BETWEEN {min_zoom} AND {max_zoom}")
            } else {
                format!(" AND {prefix}zoom_level >= {min_zoom}")
            }
        } else if let Some(max_zoom) = self.options.max_zoom {
            format!(" AND {prefix}zoom_level <= {max_zoom}")
        } else {
            String::new()
        };

        if !self.options.bbox.is_empty() {
            sql.push_str(" AND (\n");
            for (idx, bbox) in self.options.bbox.iter().enumerate() {
                // Use maximum zoom value for easy filtering,
                // converting it on the fly to the actual zoom level
                let (min_x, min_y, max_x, max_y) =
                    bbox_to_xyz(bbox.left, bbox.bottom, bbox.right, bbox.top, MAX_ZOOM);
                trace!(
                    "Bounding box {bbox} converted to {min_x},{min_y},{max_x},{max_y} at zoom {MAX_ZOOM}"
                );
                let (min_y, max_y) = (
                    invert_y_value(MAX_ZOOM, max_y),
                    invert_y_value(MAX_ZOOM, min_y),
                );

                if idx > 0 {
                    sql.push_str(" OR\n");
                }
                writeln!(
                    sql,
                    "(({prefix}tile_column * (1 << ({MAX_ZOOM} - {prefix}zoom_level))) BETWEEN {min_x} AND {max_x} \
                     AND ({prefix}tile_row * (1 << ({MAX_ZOOM} - {prefix}zoom_level))) BETWEEN {min_y} AND {max_y})",
                ).unwrap();
            }
            sql.push(')');
        }

        sql
    }
}

fn get_select_from_apply_patch(
    src_type: MbtType,
    dif_info: &PatchFileInfo,
    dst_type: MbtType,
) -> String {
    fn query_for_dst(frm_db: &'static str, frm_type: MbtType, to_type: MbtType) -> String {
        match to_type {
            Flat => format!("{frm_db}.tiles"),
            FlatWithHash | Normalized { .. } => match frm_type {
                Flat => format!(
                    "
        (SELECT zoom_level, tile_column, tile_row, tile_data, md5_hex(tile_data) AS tile_hash
         FROM {frm_db}.tiles)"
                ),
                FlatWithHash => format!("{frm_db}.tiles_with_hash"),
                Normalized { hash_view } => {
                    if hash_view {
                        format!("{frm_db}.tiles_with_hash")
                    } else {
                        format!(
                            "
        (SELECT zoom_level, tile_column, tile_row, tile_data, map.tile_id AS tile_hash
        FROM {frm_db}.map JOIN {frm_db}.images ON map.tile_id = images.tile_id)"
                        )
                    }
                }
            },
        }
    }

    let tile_hash_expr = if dst_type == Flat {
        String::new()
    } else {
        fn get_tile_hash_expr(tbl: &str, typ: MbtType) -> String {
            match typ {
                Flat => format!("IIF({tbl}.tile_data ISNULL, NULL, md5_hex({tbl}.tile_data))"),
                FlatWithHash | Normalized { .. } => format!("{tbl}.tile_hash"),
            }
        }

        format!(
            ", COALESCE({}, {}) as tile_hash",
            get_tile_hash_expr("difTiles", dif_info.mbt_type),
            get_tile_hash_expr("srcTiles", src_type)
        )
    };

    let src_tiles = query_for_dst("sourceDb", src_type, dst_type);
    let diff_tiles = query_for_dst("diffDb", dif_info.mbt_type, dst_type);

    let (bindiff_from, bindiff_cond) = if let Some(patch_type) = dif_info.patch_type {
        // do not copy any tiles that are in the patch table
        let tbl = get_bsdiff_tbl_name(patch_type);
        (
            format!(
                "
             LEFT JOIN diffDb.{tbl} AS bdTbl
               ON bdTbl.zoom_level = srcTiles.zoom_level
                 AND bdTbl.tile_column = srcTiles.tile_column
                 AND bdTbl.tile_row = srcTiles.tile_row"
            ),
            "AND bdTbl.patch_data ISNULL",
        )
    } else {
        (String::new(), "")
    };

    // Take dif tile_data if it is set, otherwise take the one from src
    // Skip tiles if src and dif both have a matching index, but the dif tile_data is NULL
    format!(
        "
        SELECT COALESCE(srcTiles.zoom_level, difTiles.zoom_level) as zoom_level
             , COALESCE(srcTiles.tile_column, difTiles.tile_column) as tile_column
             , COALESCE(srcTiles.tile_row, difTiles.tile_row) as tile_row
             , COALESCE(difTiles.tile_data, srcTiles.tile_data) as tile_data
             {tile_hash_expr}
        FROM {src_tiles} AS srcTiles FULL JOIN {diff_tiles} AS difTiles
             ON srcTiles.zoom_level = difTiles.zoom_level
               AND srcTiles.tile_column = difTiles.tile_column
               AND srcTiles.tile_row = difTiles.tile_row
             {bindiff_from}
        WHERE (difTiles.zoom_level ISNULL OR difTiles.tile_data NOTNULL) {bindiff_cond}"
    )
}

fn get_select_from_with_diff(
    dif_type: MbtType,
    dst_type: MbtType,
    patch_type: Option<PatchType>,
) -> String {
    let tile_hash_expr;
    let diff_tiles;
    if dst_type == Flat {
        tile_hash_expr = "";
        diff_tiles = "diffDb.tiles";
    } else {
        tile_hash_expr = match dif_type {
            Flat => ", COALESCE(md5_hex(difTiles.tile_data), '') as tile_hash",
            FlatWithHash | Normalized { .. } => ", COALESCE(difTiles.tile_hash, '') as tile_hash",
        };
        diff_tiles = match dif_type {
            Flat => "diffDb.tiles",
            FlatWithHash => "diffDb.tiles_with_hash",
            Normalized { .. } => {
                "
        (SELECT zoom_level, tile_column, tile_row, tile_data, map.tile_id AS tile_hash
        FROM diffDb.map JOIN diffDb.images ON diffDb.map.tile_id = diffDb.images.tile_id)"
            }
        };
    }

    let sql_cond = if patch_type.is_some() {
        ""
    } else {
        "OR srcTiles.tile_data != difTiles.tile_data"
    };
    format!(
        "
        SELECT COALESCE(srcTiles.zoom_level, difTiles.zoom_level) as zoom_level
             , COALESCE(srcTiles.tile_column, difTiles.tile_column) as tile_column
             , COALESCE(srcTiles.tile_row, difTiles.tile_row) as tile_row
             , difTiles.tile_data as tile_data
             {tile_hash_expr}
        FROM sourceDb.tiles AS srcTiles FULL JOIN {diff_tiles} AS difTiles
             ON srcTiles.zoom_level = difTiles.zoom_level
               AND srcTiles.tile_column = difTiles.tile_column
               AND srcTiles.tile_row = difTiles.tile_row
        WHERE (srcTiles.tile_data ISNULL
               OR difTiles.tile_data ISNULL
               {sql_cond})"
    )
}

fn get_select_from(src_type: MbtType, dst_type: MbtType) -> &'static str {
    if dst_type == Flat {
        "SELECT zoom_level, tile_column, tile_row, tile_data FROM sourceDb.tiles WHERE TRUE"
    } else {
        match src_type {
            Flat => {
                "
        SELECT zoom_level, tile_column, tile_row, tile_data, md5_hex(tile_data) as tile_hash
        FROM sourceDb.tiles
        WHERE TRUE"
            }
            FlatWithHash => {
                "
        SELECT zoom_level, tile_column, tile_row, tile_data, tile_hash
        FROM sourceDb.tiles_with_hash
        WHERE TRUE"
            }
            Normalized { .. } => {
                "
        SELECT zoom_level, tile_column, tile_row, tile_data, map.tile_id AS tile_hash
        FROM sourceDb.map JOIN sourceDb.images
          ON sourceDb.map.tile_id = sourceDb.images.tile_id
        WHERE TRUE"
            }
        }
    }
}

fn patch_type_str(patch_type: Option<PatchType>) -> &'static str {
    if let Some(v) = patch_type {
        match v {
            BinDiffGz => " with bin-diff on gzip-ed tiles",
            BinDiffRaw => " with bin-diff-raw",
        }
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use sqlx::{Decode, Sqlite, SqliteConnection, Type};

    use super::*;

    // TODO: Most of these tests are duplicating the tests from tests/mbtiles.rs, and should be cleaned up/removed.

    const FLAT: Option<MbtTypeCli> = Some(MbtTypeCli::Flat);
    const FLAT_WITH_HASH: Option<MbtTypeCli> = Some(MbtTypeCli::FlatWithHash);
    const NORM_CLI: Option<MbtTypeCli> = Some(MbtTypeCli::Normalized);
    const NORM_WITH_VIEW: MbtType = Normalized { hash_view: true };

    async fn get_one<T>(conn: &mut SqliteConnection, sql: &str) -> T
    where
        for<'r> T: Decode<'r, Sqlite> + Type<Sqlite>,
    {
        query(sql).fetch_one(conn).await.unwrap().get::<T, _>(0)
    }

    async fn verify_copy_all(
        src_filepath: PathBuf,
        dst_filepath: PathBuf,
        dst_type_cli: Option<MbtTypeCli>,
        expected_dst_type: MbtType,
    ) -> MbtResult<()> {
        let opt = MbtilesCopier {
            src_file: src_filepath.clone(),
            dst_file: dst_filepath.clone(),
            dst_type_cli,
            ..Default::default()
        };
        let mut dst_conn = opt.run().await?;

        Mbtiles::new(src_filepath)?
            .attach_to(&mut dst_conn, "testSrcDb")
            .await?;

        assert_eq!(
            Mbtiles::new(dst_filepath)?
                .detect_type(&mut dst_conn)
                .await?,
            expected_dst_type
        );

        assert!(
            dst_conn
                .fetch_optional("SELECT * FROM testSrcDb.tiles EXCEPT SELECT * FROM tiles")
                .await?
                .is_none()
        );

        Ok(())
    }

    async fn verify_copy_with_zoom_filter(
        opt: MbtilesCopier,
        expected_zoom_levels: u8,
    ) -> MbtResult<()> {
        let mut dst_conn = opt.run().await?;

        assert_eq!(
            get_one::<u8>(
                &mut dst_conn,
                "SELECT COUNT(DISTINCT zoom_level) FROM tiles;"
            )
            .await,
            expected_zoom_levels
        );

        Ok(())
    }

    #[actix_rt::test]
    async fn copy_flat_tables() -> MbtResult<()> {
        let src = PathBuf::from("../tests/fixtures/mbtiles/world_cities.mbtiles");
        let dst = PathBuf::from("file:copy_flat_tables_mem_db?mode=memory&cache=shared");
        verify_copy_all(src, dst, None, Flat).await
    }

    #[actix_rt::test]
    async fn copy_flat_from_flat_with_hash_tables() -> MbtResult<()> {
        let src = PathBuf::from("../tests/fixtures/mbtiles/zoomed_world_cities.mbtiles");
        let dst = PathBuf::from(
            "file:copy_flat_from_flat_with_hash_tables_mem_db?mode=memory&cache=shared",
        );
        verify_copy_all(src, dst, FLAT, Flat).await
    }

    #[actix_rt::test]
    async fn copy_flat_from_normalized_tables() -> MbtResult<()> {
        let src = PathBuf::from("../tests/fixtures/mbtiles/geography-class-png.mbtiles");
        let dst =
            PathBuf::from("file:copy_flat_from_normalized_tables_mem_db?mode=memory&cache=shared");
        verify_copy_all(src, dst, FLAT, Flat).await
    }

    #[actix_rt::test]
    async fn copy_flat_with_hash_tables() -> MbtResult<()> {
        let src = PathBuf::from("../tests/fixtures/mbtiles/zoomed_world_cities.mbtiles");
        let dst = PathBuf::from("file:copy_flat_with_hash_tables_mem_db?mode=memory&cache=shared");
        verify_copy_all(src, dst, None, FlatWithHash).await
    }

    #[actix_rt::test]
    async fn copy_flat_with_hash_from_flat_tables() -> MbtResult<()> {
        let src = PathBuf::from("../tests/fixtures/mbtiles/world_cities.mbtiles");
        let dst = PathBuf::from(
            "file:copy_flat_with_hash_from_flat_tables_mem_db?mode=memory&cache=shared",
        );
        verify_copy_all(src, dst, FLAT_WITH_HASH, FlatWithHash).await
    }

    #[actix_rt::test]
    async fn copy_flat_with_hash_from_normalized_tables() -> MbtResult<()> {
        let src = PathBuf::from("../tests/fixtures/mbtiles/geography-class-png.mbtiles");
        let dst = PathBuf::from(
            "file:copy_flat_with_hash_from_normalized_tables_mem_db?mode=memory&cache=shared",
        );
        verify_copy_all(src, dst, FLAT_WITH_HASH, FlatWithHash).await
    }

    #[actix_rt::test]
    async fn copy_normalized_tables() -> MbtResult<()> {
        let src = PathBuf::from("../tests/fixtures/mbtiles/geography-class-png.mbtiles");
        let dst = PathBuf::from("file:copy_normalized_tables_mem_db?mode=memory&cache=shared");
        verify_copy_all(src, dst, None, NORM_WITH_VIEW).await
    }

    #[actix_rt::test]
    async fn copy_normalized_from_flat_tables() -> MbtResult<()> {
        let src = PathBuf::from("../tests/fixtures/mbtiles/world_cities.mbtiles");
        let dst =
            PathBuf::from("file:copy_normalized_from_flat_tables_mem_db?mode=memory&cache=shared");
        verify_copy_all(src, dst, NORM_CLI, NORM_WITH_VIEW).await
    }

    #[actix_rt::test]
    async fn copy_normalized_from_flat_with_hash_tables() -> MbtResult<()> {
        let src = PathBuf::from("../tests/fixtures/mbtiles/zoomed_world_cities.mbtiles");
        let dst = PathBuf::from(
            "file:copy_normalized_from_flat_with_hash_tables_mem_db?mode=memory&cache=shared",
        );
        verify_copy_all(src, dst, NORM_CLI, NORM_WITH_VIEW).await
    }

    #[actix_rt::test]
    async fn copy_with_min_max_zoom() -> MbtResult<()> {
        let opt = MbtilesCopier {
            src_file: PathBuf::from("../tests/fixtures/mbtiles/world_cities.mbtiles"),
            dst_file: PathBuf::from("file:copy_with_min_max_zoom_mem_db?mode=memory&cache=shared"),
            min_zoom: Some(2),
            max_zoom: Some(4),
            ..Default::default()
        };
        verify_copy_with_zoom_filter(opt, 3).await
    }

    #[actix_rt::test]
    async fn copy_with_zoom_levels() -> MbtResult<()> {
        let opt = MbtilesCopier {
            src_file: PathBuf::from("../tests/fixtures/mbtiles/world_cities.mbtiles"),
            dst_file: PathBuf::from("file:copy_with_zoom_levels_mem_db?mode=memory&cache=shared"),
            min_zoom: Some(2),
            max_zoom: Some(4),
            zoom_levels: vec![1, 6],
            ..Default::default()
        };
        verify_copy_with_zoom_filter(opt, 2).await
    }

    #[actix_rt::test]
    async fn copy_with_diff_with_file() -> MbtResult<()> {
        let src = PathBuf::from("../tests/fixtures/mbtiles/geography-class-jpg.mbtiles");
        let dst = PathBuf::from("file:copy_with_diff_with_file_mem_db?mode=memory&cache=shared");

        let diff_file =
            PathBuf::from("../tests/fixtures/mbtiles/geography-class-jpg-modified.mbtiles");

        let opt = MbtilesCopier {
            src_file: src.clone(),
            dst_file: dst.clone(),
            diff_with_file: Some((diff_file.clone(), None)),
            force: true,
            ..Default::default()
        };
        let mut dst_conn = opt.run().await?;

        assert!(
            dst_conn
                .fetch_optional("SELECT 1 FROM sqlite_schema WHERE name = 'tiles';")
                .await?
                .is_some()
        );

        assert_eq!(
            get_one::<i32>(&mut dst_conn, "SELECT COUNT(*) FROM map;").await,
            3
        );

        assert!(
            get_one::<Option<i32>>(
                &mut dst_conn,
                "SELECT * FROM tiles WHERE zoom_level = 2 AND tile_row = 2 AND tile_column = 2;"
            )
            .await
            .is_some()
        );

        assert!(
            get_one::<Option<i32>>(
                &mut dst_conn,
                "SELECT * FROM tiles WHERE zoom_level = 1 AND tile_row = 1 AND tile_column = 1;"
            )
            .await
            .is_some()
        );

        assert!(
            get_one::<Option<i32>>(
                &mut dst_conn,
                "SELECT * FROM map WHERE zoom_level = 0 AND tile_row = 0 AND tile_column = 0;",
            )
            .await
            .is_some()
        );

        Ok(())
    }

    #[actix_rt::test]
    async fn copy_to_existing_abort_mode() {
        let src = PathBuf::from("../tests/fixtures/mbtiles/world_cities_modified.mbtiles");
        let dst = PathBuf::from("../tests/fixtures/mbtiles/world_cities.mbtiles");

        let opt = MbtilesCopier {
            src_file: src.clone(),
            dst_file: dst.clone(),
            on_duplicate: Some(CopyDuplicateMode::Abort),
            ..Default::default()
        };

        assert!(matches!(
            opt.run().await.unwrap_err(),
            MbtError::RusqliteError(..)
        ));
    }

    #[actix_rt::test]
    async fn copy_to_existing_override_mode() -> MbtResult<()> {
        let src_file = PathBuf::from("../tests/fixtures/mbtiles/world_cities_modified.mbtiles");

        // Copy the dst file to an in-memory DB
        let dst_file = PathBuf::from("../tests/fixtures/mbtiles/world_cities.mbtiles");
        let dst =
            PathBuf::from("file:copy_to_existing_override_mode_mem_db?mode=memory&cache=shared");

        let _dst_conn = MbtilesCopier {
            src_file: dst_file.clone(),
            dst_file: dst.clone(),
            ..Default::default()
        }
        .run()
        .await?;

        let opt = MbtilesCopier {
            src_file: src_file.clone(),
            dst_file: dst.clone(),
            on_duplicate: Some(CopyDuplicateMode::Override),
            ..Default::default()
        };
        let mut dst_conn = opt.run().await?;

        // Verify the tiles in the destination file is a superset of the tiles in the source file
        Mbtiles::new(src_file)?
            .attach_to(&mut dst_conn, "testOtherDb")
            .await?;
        assert!(
            dst_conn
                .fetch_optional("SELECT * FROM testOtherDb.tiles EXCEPT SELECT * FROM tiles;")
                .await?
                .is_none()
        );

        Ok(())
    }

    #[actix_rt::test]
    async fn copy_to_existing_ignore_mode() -> MbtResult<()> {
        let src_file = PathBuf::from("../tests/fixtures/mbtiles/world_cities_modified.mbtiles");

        // Copy the dst file to an in-memory DB
        let dst_file = PathBuf::from("../tests/fixtures/mbtiles/world_cities.mbtiles");
        let dst =
            PathBuf::from("file:copy_to_existing_ignore_mode_mem_db?mode=memory&cache=shared");

        let _dst_conn = MbtilesCopier {
            src_file: dst_file.clone(),
            dst_file: dst.clone(),
            ..Default::default()
        }
        .run()
        .await?;

        let opt = MbtilesCopier {
            src_file: src_file.clone(),
            dst_file: dst.clone(),
            on_duplicate: Some(CopyDuplicateMode::Ignore),
            ..Default::default()
        };
        let mut dst_conn = opt.run().await?;

        // Verify the tiles in the destination file are the same as those in the source file except for those with duplicate (zoom_level, tile_column, tile_row)
        Mbtiles::new(src_file)?
            .attach_to(&mut dst_conn, "testSrcDb")
            .await?;
        Mbtiles::new(dst_file)?
            .attach_to(&mut dst_conn, "testOriginalDb")
            .await?;

        // Create a temporary table with all the tiles in the original database and
        // all the tiles in the source database except for those that conflict with tiles in the original database
        dst_conn.execute(
            "CREATE TEMP TABLE expected_tiles AS
                   SELECT COALESCE(t1.zoom_level, t2.zoom_level) as zoom_level,
                          COALESCE(t1.tile_column, t2.zoom_level) as tile_column,
                          COALESCE(t1.tile_row, t2.tile_row) as tile_row,
                          COALESCE(t1.tile_data, t2.tile_data) as tile_data
                   FROM testOriginalDb.tiles as t1
                   FULL OUTER JOIN testSrcDb.tiles as t2
                       ON t1.zoom_level = t2.zoom_level AND t1.tile_column = t2.tile_column AND t1.tile_row = t2.tile_row")
            .await?;

        // Ensure all entries in expected_tiles are in tiles and vice versa
        assert!(
            query(
                "SELECT * FROM expected_tiles EXCEPT SELECT * FROM tiles
               UNION
             SELECT * FROM tiles EXCEPT SELECT * FROM expected_tiles"
            )
            .fetch_optional(&mut dst_conn)
            .await?
            .is_none()
        );

        Ok(())
    }
}

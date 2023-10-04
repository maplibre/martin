use std::collections::HashSet;
use std::path::PathBuf;

#[cfg(feature = "cli")]
use clap::{builder::ValueParser, error::ErrorKind, Args, ValueEnum};
use enum_display::EnumDisplay;
use log::{debug, info};
use sqlite_hashes::rusqlite;
use sqlite_hashes::rusqlite::params_from_iter;
use sqlx::{query, Executor as _, Row, SqliteConnection};

use crate::errors::MbtResult;
use crate::mbtiles::MbtType;
use crate::mbtiles::MbtType::{Flat, FlatWithHash, Normalized};
use crate::queries::{
    create_flat_tables, create_flat_with_hash_tables, create_normalized_tables,
    create_tiles_with_hash_view, detach_db, is_empty_database,
};
use crate::{MbtError, Mbtiles, AGG_TILES_HASH, AGG_TILES_HASH_IN_DIFF};

#[derive(PartialEq, Eq, Default, Debug, Clone, EnumDisplay)]
#[enum_display(case = "Kebab")]
#[cfg_attr(feature = "cli", derive(ValueEnum))]
pub enum CopyDuplicateMode {
    #[default]
    Override,
    Ignore,
    Abort,
}

#[derive(Clone, Default, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "cli", derive(Args))]
pub struct MbtilesCopier {
    /// MBTiles file to read from
    pub src_file: PathBuf,
    /// MBTiles file to write to
    pub dst_file: PathBuf,
    /// Output format of the destination file, ignored if the file exists. If not specified, defaults to the type of source
    #[cfg_attr(feature = "cli", arg(long, value_enum))]
    pub dst_type: Option<MbtType>,
    /// Specify copying behaviour when tiles with duplicate (zoom_level, tile_column, tile_row) values are found
    #[cfg_attr(feature = "cli", arg(long, value_enum, default_value_t = CopyDuplicateMode::default()))]
    pub on_duplicate: CopyDuplicateMode,
    /// Minimum zoom level to copy
    #[cfg_attr(feature = "cli", arg(long, conflicts_with("zoom_levels")))]
    pub min_zoom: Option<u8>,
    /// Maximum zoom level to copy
    #[cfg_attr(feature = "cli", arg(long, conflicts_with("zoom_levels")))]
    pub max_zoom: Option<u8>,
    /// List of zoom levels to copy
    #[cfg_attr(feature = "cli", arg(long, value_parser(ValueParser::new(HashSetValueParser{})), default_value=""))]
    pub zoom_levels: HashSet<u8>,
    /// Compare source file with this file, and only copy non-identical tiles to destination.
    /// It should be later possible to run `mbtiles apply-diff SRC_FILE DST_FILE` to get the same DIFF file.
    #[cfg_attr(feature = "cli", arg(long))]
    pub diff_with_file: Option<PathBuf>,
    /// Skip generating a global hash for mbtiles validation. By default, `mbtiles` will compute `agg_tiles_hash` metadata value.
    #[cfg_attr(feature = "cli", arg(long))]
    pub skip_agg_tiles_hash: bool,
}

#[cfg(feature = "cli")]
#[derive(Clone)]
struct HashSetValueParser;

#[cfg(feature = "cli")]
impl clap::builder::TypedValueParser for HashSetValueParser {
    type Value = HashSet<u8>;

    fn parse_ref(
        &self,
        _cmd: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let mut result = HashSet::<u8>::new();
        let values = value
            .to_str()
            .ok_or(clap::Error::new(ErrorKind::ValueValidation))?
            .trim();
        if !values.is_empty() {
            for val in values.split(',') {
                result.insert(
                    val.trim()
                        .parse::<u8>()
                        .map_err(|_| clap::Error::new(ErrorKind::ValueValidation))?,
                );
            }
        }
        Ok(result)
    }
}

#[derive(Clone, Debug)]
struct MbtileCopierInt {
    src_mbtiles: Mbtiles,
    dst_mbtiles: Mbtiles,
    options: MbtilesCopier,
}

impl MbtilesCopier {
    #[must_use]
    pub fn new(src_filepath: PathBuf, dst_filepath: PathBuf) -> Self {
        Self {
            src_file: src_filepath,
            dst_file: dst_filepath,
            zoom_levels: HashSet::new(),
            dst_type: None,
            on_duplicate: CopyDuplicateMode::Override,
            min_zoom: None,
            max_zoom: None,
            diff_with_file: None,
            skip_agg_tiles_hash: false,
        }
    }

    pub async fn run(self) -> MbtResult<SqliteConnection> {
        MbtileCopierInt::new(self)?.run().await
    }
}

impl MbtileCopierInt {
    pub fn new(options: MbtilesCopier) -> MbtResult<Self> {
        // We may want to resolve the files to absolute paths here, but will need to avoid various non-file cases
        if options.src_file == options.dst_file {
            return Err(MbtError::SameSourceAndDestination(options.src_file));
        }
        if let Some(diff_file) = &options.diff_with_file {
            if options.src_file == *diff_file || options.dst_file == *diff_file {
                return Err(MbtError::SameDiffAndSourceOrDestination(options.src_file));
            }
        }
        Ok(MbtileCopierInt {
            src_mbtiles: Mbtiles::new(&options.src_file)?,
            dst_mbtiles: Mbtiles::new(&options.dst_file)?,
            options,
        })
    }

    pub async fn run(self) -> MbtResult<SqliteConnection> {
        // src and diff file connections are not needed later, as they will be attached to the dst file
        let src_mbt = &self.src_mbtiles;
        let dst_mbt = &self.dst_mbtiles;

        let src_type = src_mbt.open_and_detect_type().await?;
        let dif = if let Some(dif_file) = &self.options.diff_with_file {
            let dif_file = Mbtiles::new(dif_file)?;
            let dif_type = dif_file.open_and_detect_type().await?;
            Some((dif_file, dif_type))
        } else {
            None
        };

        let mut conn = dst_mbt.open_or_new().await?;
        let is_empty_db = is_empty_database(&mut conn).await?;
        src_mbt.attach_to(&mut conn, "sourceDb").await?;

        let dst_type;
        if let Some((dif_mbt, dif_type)) = &dif {
            if !is_empty_db {
                return Err(MbtError::NonEmptyTargetFile(self.options.dst_file));
            }
            dst_type = self.options.dst_type.unwrap_or(src_type);
            dif_mbt.attach_to(&mut conn, "diffDb").await?;
            let dif_path = dif_mbt.filepath();
            info!("Comparing {src_mbt} ({src_type}) and {dif_path} ({dif_type}) into a new file {dst_mbt} ({dst_type})");
        } else if is_empty_db {
            dst_type = self.options.dst_type.unwrap_or(src_type);
            info!("Copying {src_mbt} ({src_type}) to a new file {dst_mbt} ({dst_type})");
        } else {
            dst_type = dst_mbt.detect_type(&mut conn).await?;
            info!("Copying {src_mbt} ({src_type}) to an existing file {dst_mbt} ({dst_type})");
        }

        if is_empty_db {
            self.init_new_schema(&mut conn, src_type, dst_type).await?;
        }

        let (on_dupl, sql_cond) = self.get_on_duplicate_sql(dst_type);

        let (select_from, query_args) = {
            let select_from = if let Some((_, dif_type)) = &dif {
                Self::get_select_from_with_diff(dst_type, *dif_type)
            } else {
                Self::get_select_from(dst_type, src_type).to_string()
            };

            let (options_sql, query_args) = self.get_options_sql();

            (format!("{select_from} {options_sql}"), query_args)
        };

        {
            debug!("Copying tiles with 'INSERT {on_dupl}' {src_type} -> {dst_type} ({sql_cond})");
            // Make sure not to execute any other queries while the handle is locked
            let mut handle_lock = conn.lock_handle().await?;
            let handle = handle_lock.as_raw_handle().as_ptr();

            // SAFETY: this is safe as long as handle_lock is valid
            let rusqlite_conn = unsafe { rusqlite::Connection::from_handle(handle) }?;

            match dst_type {
                Flat => {
                    let sql = format!("INSERT {on_dupl} INTO tiles {select_from} {sql_cond}");
                    debug!("Copying to {dst_type} with {sql} {query_args:?}");
                    rusqlite_conn.execute(&sql, params_from_iter(query_args))?
                }
                FlatWithHash => {
                    let sql =
                        format!("INSERT {on_dupl} INTO tiles_with_hash {select_from} {sql_cond}");
                    debug!("Copying to {dst_type} with {sql} {query_args:?}");
                    rusqlite_conn.execute(&sql, params_from_iter(query_args))?
                }
                Normalized => {
                    let sql = format!(
                        "INSERT {on_dupl} INTO map (zoom_level, tile_column, tile_row, tile_id)
                         SELECT zoom_level, tile_column, tile_row, hash as tile_id
                         FROM ({select_from} {sql_cond})"
                    );
                    debug!("Copying to {dst_type} with {sql} {query_args:?}");
                    rusqlite_conn.execute(&sql, params_from_iter(&query_args))?;
                    let sql = format!(
                        "INSERT OR IGNORE INTO images (tile_id, tile_data)
                         SELECT hash as tile_id, tile_data
                         FROM ({select_from})"
                    );
                    debug!("Copying to {dst_type} with {sql} {query_args:?}");
                    rusqlite_conn.execute(&sql, params_from_iter(query_args))?
                }
            };

            let sql = if self.options.diff_with_file.is_some() {
                debug!("Copying metadata with 'INSERT {on_dupl}', taking into account diff file");
                // Insert all rows from diffDb.metadata if they do not exist or are different in sourceDb.metadata.
                // Also insert all names from sourceDb.metadata that do not exist in diffDb.metadata, with their value set to NULL.
                // Rename agg_tiles_hash to agg_tiles_hash_in_diff because agg_tiles_hash will be auto-added later
                format!(
                    "INSERT {on_dupl} INTO metadata (name, value)
                         SELECT CASE WHEN dif.name = '{AGG_TILES_HASH}' THEN '{AGG_TILES_HASH_IN_DIFF}' ELSE dif.name END as name,
                                dif.value as value
                         FROM diffDb.metadata AS dif LEFT JOIN sourceDb.metadata AS src
                           ON dif.name = src.name
                         WHERE (dif.value != src.value OR src.value ISNULL)
                           AND dif.name != '{AGG_TILES_HASH_IN_DIFF}'
                     UNION ALL
                         SELECT src.name as name, NULL as value
                         FROM sourceDb.metadata AS src LEFT JOIN diffDb.metadata AS dif
                           ON src.name = dif.name
                         WHERE dif.value ISNULL AND src.name NOT IN ('{AGG_TILES_HASH}', '{AGG_TILES_HASH_IN_DIFF}');"
                )
            } else {
                debug!("Copying metadata with 'INSERT {on_dupl}'");
                format!("INSERT {on_dupl} INTO metadata SELECT name, value FROM sourceDb.metadata")
            };
            rusqlite_conn.execute(&sql, [])?;
        }

        if !self.options.skip_agg_tiles_hash {
            dst_mbt.update_agg_tiles_hash(&mut conn).await?;
        }

        detach_db(&mut conn, "sourceDb").await?;
        // Ignore error because we might not have attached diffDb
        let _ = detach_db(&mut conn, "diffDb").await;

        Ok(conn)
    }

    async fn init_new_schema(
        &self,
        conn: &mut SqliteConnection,
        src: MbtType,
        dst: MbtType,
    ) -> MbtResult<()> {
        debug!("Resetting PRAGMA settings and vacuuming");
        query!("PRAGMA page_size = 512").execute(&mut *conn).await?;
        query!("PRAGMA encoding = 'UTF-8'")
            .execute(&mut *conn)
            .await?;
        query!("VACUUM").execute(&mut *conn).await?;

        if src == dst {
            // DB objects must be created in a specific order: tables, views, triggers, indexes.
            debug!("Copying DB schema verbatim");
            let sql_objects = conn
                .fetch_all(
                    "SELECT sql
                     FROM sourceDb.sqlite_schema
                     WHERE tbl_name IN ('metadata', 'tiles', 'map', 'images', 'tiles_with_hash')
                         AND type    IN ('table', 'view', 'trigger', 'index')
                     ORDER BY CASE
                         WHEN type = 'table' THEN 1
                         WHEN type = 'view' THEN 2
                         WHEN type = 'trigger' THEN 3
                         WHEN type = 'index' THEN 4
                         ELSE 5 END",
                )
                .await?;

            for row in sql_objects {
                query(row.get(0)).execute(&mut *conn).await?;
            }
        } else {
            match dst {
                Flat => create_flat_tables(&mut *conn).await?,
                FlatWithHash => create_flat_with_hash_tables(&mut *conn).await?,
                Normalized => create_normalized_tables(&mut *conn).await?,
            };
        };

        if dst == Normalized {
            // Some normalized mbtiles files might not have this view, so even if src == dst, it might not exist
            create_tiles_with_hash_view(&mut *conn).await?;
        }

        Ok(())
    }

    /// Returns (ON DUPLICATE SQL, WHERE condition SQL)
    fn get_on_duplicate_sql(&self, dst_type: MbtType) -> (String, String) {
        match &self.options.on_duplicate {
            CopyDuplicateMode::Override => ("OR REPLACE".to_string(), String::new()),
            CopyDuplicateMode::Ignore => ("OR IGNORE".to_string(), String::new()),
            CopyDuplicateMode::Abort => ("OR ABORT".to_string(), {
                let (main_table, tile_identifier) = match dst_type {
                    Flat => ("tiles", "tile_data"),
                    FlatWithHash => ("tiles_with_hash", "tile_data"),
                    Normalized => ("map", "tile_id"),
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
            }),
        }
    }

    fn get_select_from_with_diff(dst_type: MbtType, diff_type: MbtType) -> String {
        let (hash_col_sql, diff_tiles) = if dst_type == Flat {
            ("", "diffDb.tiles")
        } else {
            match diff_type {
                Flat => (", hex(md5(diffTiles.tile_data)) as hash", "diffDb.tiles"),
                FlatWithHash => (", diffTiles.tile_hash as hash", "diffDb.tiles_with_hash"),
                Normalized => (", diffTiles.hash",
                               "(SELECT zoom_level, tile_column, tile_row, tile_data, map.tile_id AS hash
                                 FROM diffDb.map JOIN diffDb.images ON diffDb.map.tile_id = diffDb.images.tile_id)"),
            }
        };

        format!(
            "SELECT COALESCE(sourceTiles.zoom_level, diffTiles.zoom_level) as zoom_level
                  , COALESCE(sourceTiles.tile_column, diffTiles.tile_column) as tile_column
                  , COALESCE(sourceTiles.tile_row, diffTiles.tile_row) as tile_row
                  , diffTiles.tile_data as tile_data
                  {hash_col_sql}
             FROM sourceDb.tiles AS sourceTiles FULL JOIN {diff_tiles} AS diffTiles
                  ON sourceTiles.zoom_level = diffTiles.zoom_level
                     AND sourceTiles.tile_column = diffTiles.tile_column
                     AND sourceTiles.tile_row = diffTiles.tile_row
             WHERE (sourceTiles.tile_data != diffTiles.tile_data
                    OR sourceTiles.tile_data ISNULL
                    OR diffTiles.tile_data ISNULL)"
        )
    }

    fn get_select_from(dst_type: MbtType, src_type: MbtType) -> &'static str {
        if dst_type == Flat {
            "SELECT zoom_level, tile_column, tile_row, tile_data FROM sourceDb.tiles WHERE TRUE"
        } else {
            match src_type {
                Flat => "SELECT zoom_level, tile_column, tile_row, tile_data, hex(md5(tile_data)) as hash
                         FROM sourceDb.tiles
                         WHERE TRUE",
                FlatWithHash => "SELECT zoom_level, tile_column, tile_row, tile_data, tile_hash AS hash
                                 FROM sourceDb.tiles_with_hash
                                 WHERE TRUE",
                Normalized => "SELECT zoom_level, tile_column, tile_row, tile_data, map.tile_id AS hash
                               FROM sourceDb.map JOIN sourceDb.images
                                 ON sourceDb.map.tile_id = sourceDb.images.tile_id
                               WHERE TRUE"
            }
        }
    }

    fn get_options_sql(&self) -> (String, Vec<u8>) {
        let mut query_args = vec![];

        let sql = if !&self.options.zoom_levels.is_empty() {
            for z in &self.options.zoom_levels {
                query_args.push(*z);
            }
            format!(
                " AND zoom_level IN ({})",
                vec!["?"; self.options.zoom_levels.len()].join(",")
            )
        } else if let Some(min_zoom) = self.options.min_zoom {
            if let Some(max_zoom) = self.options.max_zoom {
                query_args.push(min_zoom);
                query_args.push(max_zoom);
                " AND zoom_level BETWEEN ? AND ?".to_string()
            } else {
                query_args.push(min_zoom);
                " AND zoom_level >= ?".to_string()
            }
        } else if let Some(max_zoom) = self.options.max_zoom {
            query_args.push(max_zoom);
            " AND zoom_level <= ?".to_string()
        } else {
            String::new()
        };

        (sql, query_args)
    }
}

#[cfg(test)]
mod tests {
    use sqlx::{Decode, Sqlite, SqliteConnection, Type};

    use super::*;

    async fn get_one<T>(conn: &mut SqliteConnection, sql: &str) -> T
    where
        for<'r> T: Decode<'r, Sqlite> + Type<Sqlite>,
    {
        query(sql).fetch_one(conn).await.unwrap().get::<T, _>(0)
    }

    async fn verify_copy_all(
        src_filepath: PathBuf,
        dst_filepath: PathBuf,
        dst_type: Option<MbtType>,
        expected_dst_type: MbtType,
    ) -> MbtResult<()> {
        let mut opt = MbtilesCopier::new(src_filepath.clone(), dst_filepath.clone());
        opt.dst_type = dst_type;
        let mut dst_conn = opt.run().await?;

        Mbtiles::new(src_filepath)?
            .attach_to(&mut dst_conn, "srcDb")
            .await?;

        assert_eq!(
            Mbtiles::new(dst_filepath)?
                .detect_type(&mut dst_conn)
                .await?,
            expected_dst_type
        );

        assert!(dst_conn
            .fetch_optional("SELECT * FROM srcDb.tiles EXCEPT SELECT * FROM tiles")
            .await?
            .is_none());

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
        verify_copy_all(src, dst, Some(Flat), Flat).await
    }

    #[actix_rt::test]
    async fn copy_flat_from_normalized_tables() -> MbtResult<()> {
        let src = PathBuf::from("../tests/fixtures/mbtiles/geography-class-png.mbtiles");
        let dst =
            PathBuf::from("file:copy_flat_from_normalized_tables_mem_db?mode=memory&cache=shared");
        verify_copy_all(src, dst, Some(Flat), Flat).await
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
        verify_copy_all(src, dst, Some(FlatWithHash), FlatWithHash).await
    }

    #[actix_rt::test]
    async fn copy_flat_with_hash_from_normalized_tables() -> MbtResult<()> {
        let src = PathBuf::from("../tests/fixtures/mbtiles/geography-class-png.mbtiles");
        let dst = PathBuf::from(
            "file:copy_flat_with_hash_from_normalized_tables_mem_db?mode=memory&cache=shared",
        );
        verify_copy_all(src, dst, Some(FlatWithHash), FlatWithHash).await
    }

    #[actix_rt::test]
    async fn copy_normalized_tables() -> MbtResult<()> {
        let src = PathBuf::from("../tests/fixtures/mbtiles/geography-class-png.mbtiles");
        let dst = PathBuf::from("file:copy_normalized_tables_mem_db?mode=memory&cache=shared");
        verify_copy_all(src, dst, None, Normalized).await
    }

    #[actix_rt::test]
    async fn copy_normalized_from_flat_tables() -> MbtResult<()> {
        let src = PathBuf::from("../tests/fixtures/mbtiles/world_cities.mbtiles");
        let dst =
            PathBuf::from("file:copy_normalized_from_flat_tables_mem_db?mode=memory&cache=shared");
        verify_copy_all(src, dst, Some(Normalized), Normalized).await
    }

    #[actix_rt::test]
    async fn copy_normalized_from_flat_with_hash_tables() -> MbtResult<()> {
        let src = PathBuf::from("../tests/fixtures/mbtiles/zoomed_world_cities.mbtiles");
        let dst = PathBuf::from(
            "file:copy_normalized_from_flat_with_hash_tables_mem_db?mode=memory&cache=shared",
        );
        verify_copy_all(src, dst, Some(Normalized), Normalized).await
    }

    #[actix_rt::test]
    async fn copy_with_min_max_zoom() -> MbtResult<()> {
        let src = PathBuf::from("../tests/fixtures/mbtiles/world_cities.mbtiles");
        let dst = PathBuf::from("file:copy_with_min_max_zoom_mem_db?mode=memory&cache=shared");
        let mut opt = MbtilesCopier::new(src, dst);
        opt.min_zoom = Some(2);
        opt.max_zoom = Some(4);
        verify_copy_with_zoom_filter(opt, 3).await
    }

    #[actix_rt::test]
    async fn copy_with_zoom_levels() -> MbtResult<()> {
        let src = PathBuf::from("../tests/fixtures/mbtiles/world_cities.mbtiles");
        let dst = PathBuf::from("file:copy_with_zoom_levels_mem_db?mode=memory&cache=shared");
        let mut opt = MbtilesCopier::new(src, dst);
        opt.min_zoom = Some(2);
        opt.max_zoom = Some(4);
        opt.zoom_levels.extend(&[1, 6]);
        verify_copy_with_zoom_filter(opt, 2).await
    }

    #[actix_rt::test]
    async fn copy_with_diff_with_file() -> MbtResult<()> {
        let src = PathBuf::from("../tests/fixtures/mbtiles/geography-class-jpg.mbtiles");
        let dst = PathBuf::from("file:copy_with_diff_with_file_mem_db?mode=memory&cache=shared");

        let diff_file =
            PathBuf::from("../tests/fixtures/mbtiles/geography-class-jpg-modified.mbtiles");

        let mut opt = MbtilesCopier::new(src.clone(), dst.clone());
        opt.diff_with_file = Some(diff_file.clone());
        let mut dst_conn = opt.run().await?;

        assert!(dst_conn
            .fetch_optional("SELECT 1 FROM sqlite_schema WHERE name = 'tiles';")
            .await?
            .is_some());

        assert_eq!(
            get_one::<i32>(&mut dst_conn, "SELECT COUNT(*) FROM map;").await,
            3
        );

        assert!(get_one::<Option<i32>>(
            &mut dst_conn,
            "SELECT * FROM tiles WHERE zoom_level = 2 AND tile_row = 2 AND tile_column = 2;"
        )
        .await
        .is_some());

        assert!(get_one::<Option<i32>>(
            &mut dst_conn,
            "SELECT * FROM tiles WHERE zoom_level = 1 AND tile_row = 1 AND tile_column = 1;"
        )
        .await
        .is_some());

        assert!(get_one::<Option<i32>>(
            &mut dst_conn,
            "SELECT tile_id FROM map WHERE zoom_level = 0 AND tile_row = 0 AND tile_column = 0;"
        )
        .await
        .is_none());

        Ok(())
    }

    #[actix_rt::test]
    async fn ignore_dst_type_when_copy_to_existing() -> MbtResult<()> {
        let src_file = PathBuf::from("../tests/fixtures/mbtiles/world_cities_modified.mbtiles");

        // Copy the dst file to an in-memory DB
        let dst_file = PathBuf::from("../tests/fixtures/mbtiles/world_cities.mbtiles");
        let dst = PathBuf::from(
            "file:ignore_dst_type_when_copy_to_existing_mem_db?mode=memory&cache=shared",
        );

        let _dst_conn = MbtilesCopier::new(dst_file.clone(), dst.clone())
            .run()
            .await?;

        verify_copy_all(src_file, dst, Some(Normalized), Flat).await
    }

    #[actix_rt::test]
    async fn copy_to_existing_abort_mode() {
        let src = PathBuf::from("../tests/fixtures/mbtiles/world_cities_modified.mbtiles");
        let dst = PathBuf::from("../tests/fixtures/mbtiles/world_cities.mbtiles");

        let mut opt = MbtilesCopier::new(src.clone(), dst.clone());
        opt.on_duplicate = CopyDuplicateMode::Abort;

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

        let _dst_conn = MbtilesCopier::new(dst_file.clone(), dst.clone())
            .run()
            .await?;

        let mut dst_conn = MbtilesCopier::new(src_file.clone(), dst.clone())
            .run()
            .await?;

        // Verify the tiles in the destination file is a superset of the tiles in the source file
        Mbtiles::new(src_file)?
            .attach_to(&mut dst_conn, "otherDb")
            .await?;
        assert!(dst_conn
            .fetch_optional("SELECT * FROM otherDb.tiles EXCEPT SELECT * FROM tiles;")
            .await?
            .is_none());

        Ok(())
    }

    #[actix_rt::test]
    async fn copy_to_existing_ignore_mode() -> MbtResult<()> {
        let src_file = PathBuf::from("../tests/fixtures/mbtiles/world_cities_modified.mbtiles");

        // Copy the dst file to an in-memory DB
        let dst_file = PathBuf::from("../tests/fixtures/mbtiles/world_cities.mbtiles");
        let dst =
            PathBuf::from("file:copy_to_existing_ignore_mode_mem_db?mode=memory&cache=shared");

        let _dst_conn = MbtilesCopier::new(dst_file.clone(), dst.clone())
            .run()
            .await?;

        let mut opt = MbtilesCopier::new(src_file.clone(), dst.clone());
        opt.on_duplicate = CopyDuplicateMode::Ignore;
        let mut dst_conn = opt.run().await?;

        // Verify the tiles in the destination file are the same as those in the source file except for those with duplicate (zoom_level, tile_column, tile_row)
        Mbtiles::new(src_file)?
            .attach_to(&mut dst_conn, "srcDb")
            .await?;
        Mbtiles::new(dst_file)?
            .attach_to(&mut dst_conn, "originalDb")
            .await?;

        // Create a temporary table with all the tiles in the original database and
        // all the tiles in the source database except for those that conflict with tiles in the original database
        dst_conn.execute(
            "CREATE TEMP TABLE expected_tiles AS
                   SELECT COALESCE(t1.zoom_level, t2.zoom_level) as zoom_level,
                          COALESCE(t1.tile_column, t2.zoom_level) as tile_column,
                          COALESCE(t1.tile_row, t2.tile_row) as tile_row,
                          COALESCE(t1.tile_data, t2.tile_data) as tile_data
                   FROM originalDb.tiles as t1
                   FULL OUTER JOIN srcDb.tiles as t2
                       ON t1.zoom_level = t2.zoom_level AND t1.tile_column = t2.tile_column AND t1.tile_row = t2.tile_row")
            .await?;

        // Ensure all entries in expected_tiles are in tiles and vice versa
        assert!(query(
            "SELECT * FROM expected_tiles EXCEPT SELECT * FROM tiles
               UNION
             SELECT * FROM tiles EXCEPT SELECT * FROM expected_tiles"
        )
        .fetch_optional(&mut dst_conn)
        .await?
        .is_none());

        Ok(())
    }
}

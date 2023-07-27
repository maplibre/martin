extern crate core;

use std::collections::HashSet;
use std::path::PathBuf;

#[cfg(feature = "cli")]
use clap::{builder::ValueParser, error::ErrorKind, Args, ValueEnum};
use sqlx::sqlite::{SqliteArguments, SqliteConnectOptions};
use sqlx::{query, query_with, Arguments, Connection, Row, SqliteConnection};

use crate::errors::MbtResult;
use crate::mbtiles::MbtType;
use crate::mbtiles::MbtType::{DeDuplicated, TileTables};
use crate::{MbtError, Mbtiles};

#[derive(PartialEq, Eq, Default, Debug, Clone)]
#[cfg_attr(feature = "cli", derive(ValueEnum))]
pub enum CopyDuplicateMode {
    #[default]
    Override,
    Ignore,
    Abort,
}

#[derive(Clone, Default, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "cli", derive(Args))]
pub struct TileCopierOptions {
    /// MBTiles file to read from
    src_file: PathBuf,
    /// MBTiles file to write to
    dst_file: PathBuf,
    /// Force the output file to be in a simple MBTiles format with a `tiles` table
    ///
    #[cfg_attr(feature = "cli", arg(long))]
    force_simple: bool,
    /// Specify copying behaviour when tiles with duplicate (zoom_level, tile_column, tile_row) values are found
    #[cfg_attr(feature = "cli", arg(long, value_enum, default_value_t = CopyDuplicateMode::Override))]
    on_duplicate: CopyDuplicateMode,
    /// Minimum zoom level to copy
    #[cfg_attr(feature = "cli", arg(long, conflicts_with("zoom_levels")))]
    min_zoom: Option<u8>,
    /// Maximum zoom level to copy
    #[cfg_attr(feature = "cli", arg(long, conflicts_with("zoom_levels")))]
    max_zoom: Option<u8>,
    /// List of zoom levels to copy
    #[cfg_attr(feature = "cli", arg(long, value_parser(ValueParser::new(HashSetValueParser{})), default_value=""))]
    zoom_levels: HashSet<u8>,
    /// Compare source file with this file, and only copy non-identical tiles to destination
    #[cfg_attr(feature = "cli", arg(long, requires("force_simple")))]
    diff_with_file: Option<PathBuf>,
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
struct TileCopier {
    src_mbtiles: Mbtiles,
    dst_mbtiles: Mbtiles,
    options: TileCopierOptions,
}

impl TileCopierOptions {
    pub fn new(src_filepath: PathBuf, dst_filepath: PathBuf) -> Self {
        Self {
            src_file: src_filepath,
            dst_file: dst_filepath,
            zoom_levels: HashSet::new(),
            force_simple: false,
            on_duplicate: CopyDuplicateMode::Override,
            min_zoom: None,
            max_zoom: None,
            diff_with_file: None,
        }
    }

    pub fn force_simple(mut self, force_simple: bool) -> Self {
        self.force_simple = force_simple;
        self
    }

    pub fn on_duplicate(mut self, on_duplicate: CopyDuplicateMode) -> Self {
        self.on_duplicate = on_duplicate;
        self
    }

    pub fn zoom_levels(mut self, zoom_levels: Vec<u8>) -> Self {
        self.zoom_levels.extend(zoom_levels);
        self
    }

    pub fn min_zoom(mut self, min_zoom: Option<u8>) -> Self {
        self.min_zoom = min_zoom;
        self
    }

    pub fn max_zoom(mut self, max_zoom: Option<u8>) -> Self {
        self.max_zoom = max_zoom;
        self
    }

    pub fn diff_with_file(mut self, diff_with_file: PathBuf) -> Self {
        self.diff_with_file = Some(diff_with_file);
        self
    }
}

impl TileCopier {
    pub fn new(options: TileCopierOptions) -> MbtResult<Self> {
        Ok(TileCopier {
            src_mbtiles: Mbtiles::new(&options.src_file)?,
            dst_mbtiles: Mbtiles::new(&options.dst_file)?,
            options,
        })
    }

    pub async fn run(self) -> MbtResult<SqliteConnection> {
        let mut mbtiles_type = open_and_detect_type(&self.src_mbtiles).await?;
        let force_simple = self.options.force_simple && mbtiles_type != TileTables;

        let opt = SqliteConnectOptions::new()
            .create_if_missing(true)
            .filename(&self.options.dst_file);
        let mut conn = SqliteConnection::connect_with(&opt).await?;

        let is_empty = query!("SELECT 1 as has_rows FROM sqlite_schema LIMIT 1")
            .fetch_optional(&mut conn)
            .await?
            .is_none();

        if !is_empty && self.options.diff_with_file.is_some() {
            return Err(MbtError::NonEmptyTargetFile(self.options.dst_file));
        }

        let path = self.src_mbtiles.filepath();
        query!("ATTACH DATABASE ? AS sourceDb", path)
            .execute(&mut conn)
            .await?;

        if is_empty {
            query!("PRAGMA page_size = 512").execute(&mut conn).await?;
            query!("VACUUM").execute(&mut conn).await?;

            if force_simple {
                for statement in &["CREATE TABLE metadata (name text NOT NULL PRIMARY KEY, value text);",
                    "CREATE TABLE tiles (zoom_level integer NOT NULL, tile_column integer NOT NULL, tile_row integer NOT NULL, tile_data blob,
                    PRIMARY KEY(zoom_level, tile_column, tile_row));"] {
                    query(statement).execute(&mut conn).await?;
                }
            } else {
                // DB objects must be created in a specific order: tables, views, triggers, indexes.

                for row in query(
                    "SELECT sql 
                        FROM sourceDb.sqlite_schema 
                        WHERE tbl_name IN ('metadata', 'tiles', 'map', 'images') 
                            AND type IN ('table', 'view', 'trigger', 'index')
                        ORDER BY CASE WHEN type = 'table' THEN 1
                          WHEN type = 'view' THEN 2
                          WHEN type = 'trigger' THEN 3
                          WHEN type = 'index' THEN 4
                          ELSE 5 END",
                )
                .fetch_all(&mut conn)
                .await?
                {
                    query(row.get(0)).execute(&mut conn).await?;
                }
            };

            query("INSERT INTO metadata SELECT * FROM sourceDb.metadata")
                .execute(&mut conn)
                .await?;
        } else {
            let dst_type = open_and_detect_type(&self.dst_mbtiles).await?;

            if mbtiles_type == TileTables && dst_type == DeDuplicated {
                return Err(MbtError::UnsupportedCopyOperation{ reason: "\
                Attempted copying from a source file with simple format to a non-empty destination file with deduplicated format, which is not currently supported"
                    .to_string(),
                });
            }

            mbtiles_type = dst_type;

            if self.options.on_duplicate == CopyDuplicateMode::Abort
                && query(
                    "SELECT * FROM tiles t1 
                        JOIN sourceDb.tiles t2 
                        ON t1.zoom_level=t2.zoom_level AND t1.tile_column=t2.tile_column AND t1.tile_row=t2.tile_row AND t1.tile_data!=t2.tile_data
                        LIMIT 1",
                )
                .fetch_optional(&mut conn)
                .await?
                .is_some()
            {
                return Err(MbtError::DuplicateValues);
            }
        }

        if force_simple {
            self.copy_tile_tables(&mut conn).await?
        } else {
            match mbtiles_type {
                TileTables => self.copy_tile_tables(&mut conn).await?,
                DeDuplicated => self.copy_deduplicated(&mut conn).await?,
            }
        }

        Ok(conn)
    }

    async fn copy_tile_tables(&self, conn: &mut SqliteConnection) -> MbtResult<()> {
        if let Some(diff_with) = &self.options.diff_with_file {
            let diff_with_mbtiles = Mbtiles::new(diff_with)?;
            // Make sure file is of valid type; the specific type is irrelevant
            // because all types will be used in the same way
            open_and_detect_type(&diff_with_mbtiles).await?;

            let path = diff_with_mbtiles.filepath();
            query!("ATTACH DATABASE ? AS newDb", path)
                .execute(&mut *conn)
                .await?;

            self.run_query_with_options(
                &mut *conn,
                "INSERT INTO tiles
                        SELECT COALESCE(sourceDb.tiles.zoom_level, newDb.tiles.zoom_level) as zoom_level,
                               COALESCE(sourceDb.tiles.tile_column, newDb.tiles.tile_column) as tile_column,
                               COALESCE(sourceDb.tiles.tile_row, newDb.tiles.tile_row) as tile_row,
                               newDb.tiles.tile_data as tile_data
                        FROM sourceDb.tiles FULL JOIN newDb.tiles
                             ON sourceDb.tiles.zoom_level = newDb.tiles.zoom_level
                             AND sourceDb.tiles.tile_column = newDb.tiles.tile_column
                             AND sourceDb.tiles.tile_row = newDb.tiles.tile_row
                        WHERE (sourceDb.tiles.tile_data != newDb.tiles.tile_data
                            OR sourceDb.tiles.tile_data ISNULL
                            OR newDb.tiles.tile_data ISNULL)",
            )
            .await
        } else {
            self.run_query_with_options(
                conn,
                // Allows for adding clauses to query using "AND"
                &format!(
                    "INSERT {} INTO tiles SELECT * FROM sourceDb.tiles WHERE TRUE",
                    match &self.options.on_duplicate {
                        CopyDuplicateMode::Override => "OR REPLACE",
                        CopyDuplicateMode::Ignore | CopyDuplicateMode::Abort => "OR IGNORE",
                    }
                ),
            )
            .await
        }
    }

    async fn copy_deduplicated(&self, conn: &mut SqliteConnection) -> MbtResult<()> {
        let on_duplicate_sql = match &self.options.on_duplicate {
            CopyDuplicateMode::Override => "OR REPLACE",
            CopyDuplicateMode::Ignore | CopyDuplicateMode::Abort => "OR IGNORE",
        };

        query(&format!(
            "INSERT {on_duplicate_sql} INTO images
                SELECT images.tile_data, images.tile_id
                FROM sourceDb.images"
        ))
        .execute(&mut *conn)
        .await?;

        self.run_query_with_options(
            conn,
            // Allows for adding clauses to query using "AND"
            &format!("INSERT {on_duplicate_sql} INTO map SELECT * FROM sourceDb.map WHERE TRUE"),
        )
        .await?;

        query("DELETE FROM images WHERE tile_id NOT IN (SELECT DISTINCT tile_id FROM map)")
            .execute(&mut *conn)
            .await?;

        Ok(())
    }

    async fn run_query_with_options(
        &self,
        conn: &mut SqliteConnection,
        sql: &str,
    ) -> MbtResult<()> {
        let mut params = SqliteArguments::default();

        let sql = if !&self.options.zoom_levels.is_empty() {
            for z in &self.options.zoom_levels {
                params.add(z);
            }
            format!(
                "{sql} AND zoom_level IN ({})",
                vec!["?"; self.options.zoom_levels.len()].join(",")
            )
        } else if let Some(min_zoom) = &self.options.min_zoom {
            if let Some(max_zoom) = &self.options.max_zoom {
                params.add(min_zoom);
                params.add(max_zoom);
                format!("{sql} AND zoom_level BETWEEN ? AND ?")
            } else {
                params.add(min_zoom);
                format!("{sql} AND zoom_level >= ?")
            }
        } else if let Some(max_zoom) = &self.options.max_zoom {
            params.add(max_zoom);
            format!("{sql} AND zoom_level <= ?")
        } else {
            sql.to_string()
        };

        query_with(sql.as_str(), params).execute(conn).await?;

        Ok(())
    }
}

async fn open_and_detect_type(mbtiles: &Mbtiles) -> MbtResult<MbtType> {
    let opt = SqliteConnectOptions::new()
        .read_only(true)
        .filename(mbtiles.filepath());
    let mut conn = SqliteConnection::connect_with(&opt).await?;
    mbtiles.detect_type(&mut conn).await
}

pub async fn apply_mbtiles_diff(
    src_file: PathBuf,
    diff_file: PathBuf,
) -> MbtResult<SqliteConnection> {
    let src_mbtiles = Mbtiles::new(src_file)?;
    let diff_mbtiles = Mbtiles::new(diff_file)?;

    let opt = SqliteConnectOptions::new().filename(src_mbtiles.filepath());
    let mut conn = SqliteConnection::connect_with(&opt).await?;
    let src_type = src_mbtiles.detect_type(&mut conn).await?;

    if src_type != TileTables {
        return Err(MbtError::IncorrectDataFormat(
            src_mbtiles.filepath().to_string(),
            TileTables,
            src_type,
        ));
    }

    open_and_detect_type(&diff_mbtiles).await?;

    let path = diff_mbtiles.filepath();
    query!("ATTACH DATABASE ? AS diffDb", path)
        .execute(&mut conn)
        .await?;

    query(
        "
    DELETE FROM tiles 
    WHERE (zoom_level, tile_column, tile_row) IN 
        (SELECT zoom_level, tile_column, tile_row FROM diffDb.tiles WHERE tile_data ISNULL);",
    )
    .execute(&mut conn)
    .await?;

    query(
        "INSERT OR REPLACE INTO tiles (zoom_level, tile_column, tile_row, tile_data) 
    SELECT * FROM diffDb.tiles WHERE tile_data NOTNULL;",
    )
    .execute(&mut conn)
    .await?;

    Ok(conn)
}

pub async fn copy_mbtiles_file(opts: TileCopierOptions) -> MbtResult<SqliteConnection> {
    let tile_copier = TileCopier::new(opts)?;

    tile_copier.run().await
}

#[cfg(test)]
mod tests {
    use sqlx::{Connection, Decode, Sqlite, SqliteConnection, Type};

    use super::*;

    async fn open_sql(path: &PathBuf) -> SqliteConnection {
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(path))
            .await
            .unwrap()
    }

    async fn get_one<T>(conn: &mut SqliteConnection, sql: &str) -> T
    where
        for<'r> T: Decode<'r, Sqlite> + Type<Sqlite>,
    {
        query(sql).fetch_one(conn).await.unwrap().get::<T, _>(0)
    }

    async fn verify_copy_all(src_filepath: PathBuf, dst_filepath: PathBuf) {
        let mut dst_conn = copy_mbtiles_file(TileCopierOptions::new(
            src_filepath.clone(),
            dst_filepath.clone(),
        ))
        .await
        .unwrap();

        assert_eq!(
            get_one::<i32>(
                &mut open_sql(&src_filepath).await,
                "SELECT COUNT(*) FROM tiles;"
            )
            .await,
            get_one::<i32>(&mut dst_conn, "SELECT COUNT(*) FROM tiles;").await
        );
    }

    async fn verify_copy_with_zoom_filter(opts: TileCopierOptions, expected_zoom_levels: u8) {
        let mut dst_conn = copy_mbtiles_file(opts).await.unwrap();

        assert_eq!(
            get_one::<u8>(
                &mut dst_conn,
                "SELECT COUNT(DISTINCT zoom_level) FROM tiles;"
            )
            .await,
            expected_zoom_levels
        );
    }

    #[actix_rt::test]
    async fn copy_tile_tables() {
        let src = PathBuf::from("../tests/fixtures/files/world_cities.mbtiles");
        let dst = PathBuf::from(":memory:");
        verify_copy_all(src, dst).await;
    }

    #[actix_rt::test]
    async fn copy_deduplicated() {
        let src = PathBuf::from("../tests/fixtures/files/geography-class-png.mbtiles");
        let dst = PathBuf::from(":memory:");
        verify_copy_all(src, dst).await;
    }

    #[actix_rt::test]
    async fn copy_with_force_simple() {
        let src = PathBuf::from("../tests/fixtures/files/geography-class-jpg.mbtiles");
        let dst = PathBuf::from(":memory:");

        let copy_opts = TileCopierOptions::new(src.clone(), dst.clone()).force_simple(true);

        let mut dst_conn = copy_mbtiles_file(copy_opts).await.unwrap();

        assert!(
            query("SELECT 1 FROM sqlite_schema WHERE type='table' AND tbl_name='tiles';")
                .fetch_optional(&mut dst_conn)
                .await
                .unwrap()
                .is_some()
        );
    }

    #[actix_rt::test]
    async fn copy_with_min_max_zoom() {
        let src = PathBuf::from("../tests/fixtures/files/world_cities.mbtiles");
        let dst = PathBuf::from(":memory:");
        let opt = TileCopierOptions::new(src, dst)
            .min_zoom(Some(2))
            .max_zoom(Some(4));
        verify_copy_with_zoom_filter(opt, 3).await;
    }

    #[actix_rt::test]
    async fn copy_with_zoom_levels() {
        let src = PathBuf::from("../tests/fixtures/files/world_cities.mbtiles");
        let dst = PathBuf::from(":memory:");
        let opt = TileCopierOptions::new(src, dst)
            .min_zoom(Some(2))
            .max_zoom(Some(4))
            .zoom_levels(vec![1, 6]);
        verify_copy_with_zoom_filter(opt, 2).await;
    }

    #[actix_rt::test]
    async fn copy_with_diff_with_file() {
        let src = PathBuf::from("../tests/fixtures/files/geography-class-jpg.mbtiles");
        let dst = PathBuf::from(":memory:");

        let diff_file =
            PathBuf::from("../tests/fixtures/files/geography-class-jpg-modified.mbtiles");

        let copy_opts = TileCopierOptions::new(src.clone(), dst.clone())
            .diff_with_file(diff_file.clone())
            .force_simple(true);

        let mut dst_conn = copy_mbtiles_file(copy_opts).await.unwrap();

        assert!(
            query("SELECT 1 FROM sqlite_schema WHERE type='table' AND tbl_name='tiles';")
                .fetch_optional(&mut dst_conn)
                .await
                .unwrap()
                .is_some()
        );

        assert_eq!(
            get_one::<i32>(&mut dst_conn, "SELECT COUNT(*) FROM tiles;").await,
            3
        );

        assert_eq!(
            get_one::<i32>(
                &mut dst_conn,
                "SELECT tile_data FROM tiles WHERE zoom_level=2 AND tile_row=2 AND tile_column=2;"
            )
            .await,
            2
        );

        assert_eq!(
            get_one::<String>(
                &mut dst_conn,
                "SELECT tile_data FROM tiles WHERE zoom_level=1 AND tile_row=1 AND tile_column=1;"
            )
            .await,
            "4"
        );

        assert!(get_one::<Option<i32>>(
            &mut dst_conn,
            "SELECT tile_data FROM tiles WHERE zoom_level=0 AND tile_row=0 AND tile_column=0;"
        )
        .await
        .is_none());
    }

    #[actix_rt::test]
    async fn copy_from_simple_to_existing_deduplicated() {
        let src = PathBuf::from("../tests/fixtures/files/world_cities_modified.mbtiles");
        let dst = PathBuf::from("../tests/fixtures/files/geography-class-jpg.mbtiles");

        let copy_opts = TileCopierOptions::new(src.clone(), dst.clone());

        assert!(matches!(
            copy_mbtiles_file(copy_opts).await.unwrap_err(),
            MbtError::UnsupportedCopyOperation { .. }
        ));
    }

    #[actix_rt::test]
    async fn copy_to_existing_abort_mode() {
        let src = PathBuf::from("../tests/fixtures/files/world_cities_modified.mbtiles");
        let dst = PathBuf::from("../tests/fixtures/files/world_cities.mbtiles");

        let copy_opts =
            TileCopierOptions::new(src.clone(), dst.clone()).on_duplicate(CopyDuplicateMode::Abort);

        assert!(matches!(
            copy_mbtiles_file(copy_opts).await.unwrap_err(),
            MbtError::DuplicateValues
        ));
    }

    #[actix_rt::test]
    async fn copy_to_existing_override_mode() {
        let src_file = PathBuf::from("../tests/fixtures/files/world_cities_modified.mbtiles");

        // Copy the dst file to an in-memory DB
        let dst_file = PathBuf::from("../tests/fixtures/files/world_cities.mbtiles");
        let dst =
            PathBuf::from("file:copy_to_existing_override_mode_mem_db?mode=memory&cache=shared");

        let _dst_conn = copy_mbtiles_file(TileCopierOptions::new(dst_file.clone(), dst.clone()))
            .await
            .unwrap();

        let mut dst_conn = copy_mbtiles_file(TileCopierOptions::new(src_file.clone(), dst.clone()))
            .await
            .unwrap();

        // Verify the tiles in the destination file is a superset of the tiles in the source file
        query("ATTACH DATABASE ? AS otherDb")
            .bind(src_file.clone().to_str().unwrap())
            .execute(&mut dst_conn)
            .await
            .unwrap();

        assert!(
            query("SELECT * FROM otherDb.tiles EXCEPT SELECT * FROM tiles;")
                .fetch_optional(&mut dst_conn)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[actix_rt::test]
    async fn copy_to_existing_ignore_mode() {
        let src_file = PathBuf::from("../tests/fixtures/files/world_cities_modified.mbtiles");

        // Copy the dst file to an in-memory DB
        let dst_file = PathBuf::from("../tests/fixtures/files/world_cities.mbtiles");
        let dst =
            PathBuf::from("file:copy_to_existing_ignore_mode_mem_db?mode=memory&cache=shared");

        let _dst_conn = copy_mbtiles_file(TileCopierOptions::new(dst_file.clone(), dst.clone()))
            .await
            .unwrap();

        let mut dst_conn = copy_mbtiles_file(
            TileCopierOptions::new(src_file.clone(), dst.clone())
                .on_duplicate(CopyDuplicateMode::Ignore),
        )
        .await
        .unwrap();

        // Verify the tiles in the destination file are the same as those in the source file except for those with duplicate (zoom_level, tile_column, tile_row)
        query("ATTACH DATABASE ? AS srcDb")
            .bind(src_file.clone().to_str().unwrap())
            .execute(&mut dst_conn)
            .await
            .unwrap();
        query("ATTACH DATABASE ? AS originalDb")
            .bind(dst_file.clone().to_str().unwrap())
            .execute(&mut dst_conn)
            .await
            .unwrap();
        // Create a temporary table with all the tiles in the original database and
        // all the tiles in the source database except for those that conflict with tiles in the original database
        query("CREATE TEMP TABLE expected_tiles AS
                    SELECT COALESCE(t1.zoom_level, t2.zoom_level) as zoom_level,
                                        COALESCE(t1.tile_column, t2.zoom_level) as tile_column,
                                        COALESCE(t1.tile_row, t2.tile_row) as tile_row,
                                        COALESCE(t1.tile_data, t2.tile_data) as tile_data
                                FROM originalDb.tiles as t1 
                                FULL OUTER JOIN srcDb.tiles as t2
                                    ON t1.zoom_level=t2.zoom_level AND t1.tile_column=t2.tile_column AND t1.tile_row=t2.tile_row")
            .execute(&mut dst_conn)
            .await
            .unwrap();

        // Ensure all entries in expected_tiles are in tiles and vice versa
        assert!(query(
            "SELECT * FROM expected_tiles EXCEPT SELECT * FROM tiles
                 UNION 
                 SELECT * FROM tiles EXCEPT SELECT * FROM expected_tiles"
        )
        .fetch_optional(&mut dst_conn)
        .await
        .unwrap()
        .is_none());
    }

    #[actix_rt::test]
    async fn apply_diff_file() {
        // Copy the src file to an in-memory DB
        let src_file = PathBuf::from("../tests/fixtures/files/world_cities.mbtiles");
        let src = PathBuf::from("file::memory:?cache=shared");

        let _src_conn = copy_mbtiles_file(TileCopierOptions::new(src_file.clone(), src.clone()))
            .await
            .unwrap();

        // Apply diff to the src data in in-memory DB
        let diff_file = PathBuf::from("../tests/fixtures/files/world_cities_diff.mbtiles");
        let mut src_conn = apply_mbtiles_diff(src, diff_file).await.unwrap();

        // Verify the data is the same as the file the diff was generated from
        let path = "../tests/fixtures/files/world_cities_modified.mbtiles";
        query!("ATTACH DATABASE ? AS otherDb", path)
            .execute(&mut src_conn)
            .await
            .unwrap();

        assert!(
            query("SELECT * FROM tiles EXCEPT SELECT * FROM otherDb.tiles;")
                .fetch_optional(&mut src_conn)
                .await
                .unwrap()
                .is_none()
        );
    }
}

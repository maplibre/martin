extern crate core;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[cfg(feature = "cli")]
use clap::{builder::ValueParser, error::ErrorKind, Args, ValueEnum};
use sqlite_hashes::rusqlite::params_from_iter;
use sqlite_hashes::{register_sha256_function, rusqlite::Connection as RusqliteConnection};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{query, Connection, Row, SqliteConnection};

use crate::errors::MbtResult;
use crate::mbtiles::MbtType;
use crate::mbtiles::MbtType::{Flat, FlatHashed, Normalized};
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
    /// TODO: add documentation Output format of the destination file, ignored if the file exists. if not specified, defaults to the type of source
    #[cfg_attr(feature = "cli", arg(long, value_enum))]
    dst_mbttype: Option<MbtType>,
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
    #[cfg_attr(feature = "cli", arg(long))]
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
            dst_mbttype: None,
            on_duplicate: CopyDuplicateMode::Override,
            min_zoom: None,
            max_zoom: None,
            diff_with_file: None,
        }
    }

    pub fn dst_mbttype(mut self, dst_mbttype: Option<MbtType>) -> Self {
        self.dst_mbttype = dst_mbttype;
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
        let src_mbttype = open_and_detect_type(&self.src_mbtiles).await?;

        let mut conn = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .create_if_missing(true)
                .filename(&self.options.dst_file),
        )
        .await?;

        let is_empty = query!("SELECT 1 as has_rows FROM sqlite_schema LIMIT 1")
            .fetch_optional(&mut conn)
            .await?
            .is_none();

        let mut dst_mbttype = if is_empty {
            self.options
                .dst_mbttype
                .clone()
                .unwrap_or_else(|| src_mbttype.clone())
        } else {
            src_mbttype.clone()
        };

        if !is_empty && self.options.diff_with_file.is_some() {
            return Err(MbtError::NonEmptyTargetFile(self.options.dst_file));
        }

        if is_empty {
            self.create_new_mbtiles(&mut conn, &dst_mbttype, &src_mbttype)
                .await?;
        } else {
            dst_mbttype = open_and_detect_type(&self.dst_mbtiles).await?;
        }

        let rusqlite_conn = RusqliteConnection::open(Path::new(&self.dst_mbtiles.filepath()))?;
        register_sha256_function(&rusqlite_conn)?;
        rusqlite_conn.execute(
            "ATTACH DATABASE ? AS sourceDb",
            [self.src_mbtiles.filepath()],
        )?;

        let on_duplicate_sql = self.get_on_duplicate_sql(&dst_mbttype);

        let (select_from, query_args) = {
            let select_from = if dst_mbttype == Flat {
                "SELECT * FROM sourceDb.tiles "
            } else {
                match src_mbttype {
                    Flat => "SELECT *, hex(sha256(tile_data)) as hash FROM sourceDb.tiles ",
                    FlatHashed => "SELECT zoom_level, tile_column, tile_row, tile_data, tile_hash AS hash FROM sourceDb.hashed_tiles",
                    Normalized => "SELECT zoom_level, tile_column, tile_row, tile_data, map.tile_id AS hash FROM sourceDb.map JOIN sourceDb.images ON sourceDb.map.tile_id=sourceDb.images.tile_id"
                }
            }.to_string();

            let (options_sql, query_args) = self.get_options_sql()?;

            (
                format!("{select_from} WHERE TRUE {options_sql}"),
                query_args,
            )
        };

        match dst_mbttype {
            Flat => rusqlite_conn.execute(
                &format!(
                    "INSERT {} INTO tiles {} {}",
                    on_duplicate_sql.0, select_from, on_duplicate_sql.1
                ),
                params_from_iter(query_args),
            )?,
            FlatHashed => rusqlite_conn.execute(
                &format!(
                    "INSERT {} INTO hashed_tiles {} {}",
                    on_duplicate_sql.0, select_from, on_duplicate_sql.1
                ),
                params_from_iter(query_args),
            )?,
            Normalized => {
                rusqlite_conn.execute(
                    &format!(
                        "INSERT {} INTO map (zoom_level, tile_column, tile_row, tile_id) SELECT zoom_level, tile_column, tile_row, hash as tile_id FROM ({} {})",
                        on_duplicate_sql.0, select_from, on_duplicate_sql.1
                    ),
                    params_from_iter(&query_args),
                )?;
                rusqlite_conn.execute(
                    &format!(
                        "INSERT OR IGNORE INTO images SELECT tile_data, hash FROM ({})",
                        select_from
                    ),
                    params_from_iter(query_args),
                )?
            }
        };

        Ok(conn)
    }

    async fn create_new_mbtiles(
        &self,
        conn: &mut SqliteConnection,
        dst_mbttype: &MbtType,
        src_mbttype: &MbtType,
    ) -> MbtResult<()> {
        let path = self.src_mbtiles.filepath();
        query!("ATTACH DATABASE ? AS sourceDb", path)
            .execute(&mut *conn)
            .await?;

        query!("PRAGMA page_size = 512").execute(&mut *conn).await?;
        query!("VACUUM").execute(&mut *conn).await?;

        if dst_mbttype != src_mbttype {
            match dst_mbttype {
                Flat => self.create_flat_tables(&mut *conn).await?,
                FlatHashed => self.create_flat_hashed_tables(&mut *conn).await?,
                Normalized => self.create_normalized_tables(&mut *conn).await?,
            };
        } else {
            // DB objects must be created in a specific order: tables, views, triggers, indexes.

            for row in query(
                "SELECT sql 
                        FROM sourceDb.sqlite_schema 
                        WHERE tbl_name IN ('metadata', 'tiles', 'map', 'images', 'hashed_tiles') 
                            AND type IN ('table', 'view', 'trigger', 'index')
                        ORDER BY CASE WHEN type = 'table' THEN 1
                          WHEN type = 'view' THEN 2
                          WHEN type = 'trigger' THEN 3
                          WHEN type = 'index' THEN 4
                          ELSE 5 END",
            )
            .fetch_all(&mut *conn)
            .await?
            {
                query(row.get(0)).execute(&mut *conn).await?;
            }
        };

        query("INSERT INTO metadata SELECT * FROM sourceDb.metadata")
            .execute(&mut *conn)
            .await?;

        Ok(())
    }

    async fn create_flat_tables(&self, conn: &mut SqliteConnection) -> MbtResult<()> {
        for statement in &[
            "CREATE TABLE metadata (name text NOT NULL PRIMARY KEY, value text);",
            "CREATE TABLE tiles (zoom_level integer NOT NULL, tile_column integer NOT NULL, tile_row integer NOT NULL, tile_data blob,
                PRIMARY KEY(zoom_level, tile_column, tile_row));"] {
            query(statement).execute(&mut *conn).await?;
        }
        Ok(())
    }

    async fn create_flat_hashed_tables(&self, conn: &mut SqliteConnection) -> MbtResult<()> {
        for statement in &[
            "CREATE TABLE metadata (name text NOT NULL PRIMARY KEY, value text);",
            "CREATE TABLE hashed_tiles (zoom_level integer NOT NULL, tile_column integer NOT NULL, tile_row integer NOT NULL, tile_data blob, tile_hash text,
                PRIMARY KEY(zoom_level, tile_column, tile_row));",
            "CREATE VIEW tiles AS SELECT zoom_level, tile_column, tile_row, tile_data FROM hashed_tiles;"] {
            query(statement).execute(&mut *conn).await?;
        }
        Ok(())
    }

    async fn create_normalized_tables(&self, conn: &mut SqliteConnection) -> MbtResult<()> {
        for statement in &[
            "CREATE TABLE metadata (name text NOT NULL PRIMARY KEY, value text);",
            "CREATE TABLE map (zoom_level integer NOT NULL, tile_column integer NOT NULL, tile_row integer NOT NULL, tile_id text,
                                  PRIMARY KEY(zoom_level, tile_column, tile_row));",
            "CREATE TABLE images (tile_data blob, tile_id text NOT NULL PRIMARY KEY);",
            "CREATE VIEW tiles AS 
                SELECT map.zoom_level AS zoom_level, map.tile_column AS tile_column, map.tile_row AS tile_row, images.tile_data AS tile_data
                FROM map
                JOIN images ON images.tile_id = map.tile_id"] {
            query(statement).execute(&mut *conn).await?;
        }
        Ok(())
    }

    fn get_on_duplicate_sql(&self, mbttype: &MbtType) -> (String, String) {
        match &self.options.on_duplicate {
            CopyDuplicateMode::Override => ("OR REPLACE".to_string(), "".to_string()),
            CopyDuplicateMode::Ignore => ("OR IGNORE".to_string(), "".to_string()),
            CopyDuplicateMode::Abort => ("OR ABORT".to_string(), {
                let (main_table, tile_identifier) = match mbttype {
                    Flat => ("tiles", "tile_data"),
                    FlatHashed => ("hashed_tiles", "tile_data"),
                    Normalized => ("map", "tile_id"),
                };

                format!(
                        "AND NOT EXISTS (\
                        SELECT 1 \
                        FROM {main_table} \
                        WHERE \
                            {main_table}.zoom_level=sourceDb.{main_table}.zoom_level \
                            AND {main_table}.tile_column=sourceDb.{main_table}.tile_column \
                            AND {main_table}.tile_row=sourceDb.{main_table}.tile_row \
                            AND {main_table}.{tile_identifier}!=sourceDb.{main_table}.{tile_identifier}\
                        )"
                    )
            }),
        }
    }

    /*    async fn copy_flat_tables(&self, conn: &mut SqliteConnection) -> MbtResult<()> {
            let (query, query_args) = if let Some(diff_with) = &self.options.diff_with_file {
                let diff_with_mbtiles = Mbtiles::new(diff_with)?;
                // Make sure file is of valid type; the specific type is irrelevant
                // because all types will be used in the same way
                open_and_detect_type(&diff_with_mbtiles).await?;

                let path = diff_with_mbtiles.filepath();
                query!("ATTACH DATABASE ? AS newDb", path)
                    .execute(&mut *conn)
                    .await?;

                self.get_options_sql(
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
                )?
            } else {
                let on_duplicate_sql = self.get_on_duplicate_sql(FlatTables);

                self.get_options_sql(
                    // Allows for adding clauses to query using "AND"
                    &format!(
                        "INSERT {} INTO tiles SELECT * FROM sourceDb.tiles WHERE TRUE {}",
                        on_duplicate_sql.0, on_duplicate_sql.1
                    ),
                )?
            };

            let mut sqlite_arguments = SqliteArguments::default();
            for query_arg in query_args {
                sqlite_arguments.add(query_arg)
            }

            query_with(&query, sqlite_arguments)
                .execute(&mut *conn)
                .await?;

            Ok(())
        }

        fn copy_from_flat_to_flat_hashed_tables(&self) -> MbtResult<()> {
            let on_duplicate_sql = self.get_on_duplicate_sql(FlatHashedTables);
            let (query, query_args) = self.get_options_sql(
                // Allows for adding clauses to query using "AND"
                &format!(
                    "INSERT {} INTO hashed_tiles SELECT *, hex(sha256(tile_data))  FROM sourceDb.tiles WHERE TRUE {}",
                    on_duplicate_sql.0, on_duplicate_sql.1
                ),
            )?;

            let conn = RusqliteConnection::open(Path::new(&self.dst_mbtiles.filepath()))?;
            register_sha256_function(&conn)?;
            conn.execute(
                "ATTACH DATABASE ? AS sourceDb",
                [self.src_mbtiles.filepath()],
            )?;
            conn.execute(&query, params![query_args.as_slice()])?;

            Ok(())
        }

        fn copy_from_flat_to_normalized_tables(&self) -> MbtResult<()> {
            let on_duplicate_sql = self.get_on_duplicate_sql(NormalizedTables);

            let conn = RusqliteConnection::open(Path::new(&self.dst_mbtiles.filepath()))?;
            register_sha256_function(&conn)?;

            conn.execute(
                "ATTACH DATABASE ? AS sourceDb",
                [self.src_mbtiles.filepath()],
            )?;

            conn.execute(
                &format!(
                    "INSERT {} INTO images
                    SELECT DISTINCT tile_data, hex(sha256(tile_data))
                    FROM sourceDb.tiles",
                    on_duplicate_sql.0
                ),
                (),
            )?;

            let (query, query_args) = self.get_options_sql(
                // Allows for adding clauses to query using "AND"
                &format!(
                    "INSERT {} INTO map (zoom_level, tile_column, tile_row, tile_id) SELECT sourceDb.tiles.zoom_level, sourceDb.tiles.tile_column, sourceDb.tiles.tile_row, images.tile_id FROM sourceDb.tiles JOIN images ON sourceDb.tiles.tile_data=images.tile_data WHERE TRUE {}",
                    on_duplicate_sql.0, on_duplicate_sql.1
                ),
            )?;
            conn.execute(&query, params![query_args.as_slice()])?;
            Ok(())
        }

        async fn copy_normalized_tables(&self, conn: &mut SqliteConnection) -> MbtResult<()> {
            /*        if let Some(diff_with) = &self.options.diff_with_file {
                let diff_with_mbtiles = Mbtiles::new(diff_with)?;
                // Make sure file is of valid type; the specific type is irrelevant
                // because all types will be used in the same way
                open_and_detect_type(&diff_with_mbtiles).await?;

                let path = diff_with_mbtiles.filepath();
                query!("ATTACH DATABASE ? AS newDb", path)
                    .execute(&mut *conn)
                    .await?;

                query(&format!(
                    "INSERT INTO images
                        SELECT COALESCE(sourceDb.images.tile_id, newDb.images.tile_id) as tile_id,
                               newDb.images.tile_data as tile_data
                        FROM sourceDb.images FULL JOIN newDb.images
                                 ON sourceDb.images.tile_id = newDb.images.tile_id
                        WHERE (sourceDb.images.tile_data != newDb.images.tile_data
                             OR sourceDb.images.tile_data ISNULL
                             OR newDb.images.tile_data ISNULL)"
                ))
                .execute(&mut *conn)
                .await?;

                self.run_query_with_options(
                    &mut *conn,
                    "INSERT INTO map
                            SELECT COALESCE(sourceDb.map.zoom_level, newDb.map.zoom_level) as zoom_level,
                                   COALESCE(sourceDb.map.tile_column, newDb.map.tile_column) as tile_column,
                                   COALESCE(sourceDb.map.tile_row, newDb.map.tile_row) as tile_row,
                                   newDb.map.tile_id as tile_id,
                                   'Something'
                            FROM sourceDb.map FULL JOIN newDb.map
                                 ON sourceDb.map.zoom_level = newDb.map.zoom_level
                                 AND sourceDb.map.tile_column = newDb.map.tile_column
                                 AND sourceDb.map.tile_row = newDb.map.tile_row
                            WHERE (sourceDb.map.tile_id != newDb.map.tile_id
                                OR sourceDb.map.tile_id ISNULL
                                OR newDb.map.tile_id ISNULL)",
                ) // TODO: missing the entries where the tile_id has not changed, could insert those as well, depends on applying diff
                    .await?;

                query("DELETE FROM images WHERE tile_id IS NULL OR tile_id IN (SELECT images.tile_id FROM images LEFT JOIN map ON images.tile_id=map.tile_id WHERE map.tile_id IS NULL)")
                    .execute(&mut *conn)
                    .await?;
            } else {*/
            let on_duplicate_sql = self.get_on_duplicate_sql(NormalizedTables);

            query(&format!(
                "INSERT {} INTO images
                    SELECT images.tile_data, images.tile_id
                    FROM sourceDb.images",
                on_duplicate_sql.0
            ))
            .execute(&mut *conn)
            .await?;

            self.get_options_sql(
                // Allows for adding clauses to query using "AND"
                &format!(
                    "INSERT {} INTO map SELECT * FROM sourceDb.map WHERE TRUE {}",
                    on_duplicate_sql.0, on_duplicate_sql.1
                ),
            )?;

            query("DELETE FROM images WHERE tile_id IS NULL OR tile_id IN (SELECT images.tile_id FROM images LEFT JOIN map ON images.tile_id=map.tile_id WHERE map.tile_id IS NULL)")
                    .execute(&mut *conn)
                    .await?;

            Ok(())
        }
    */
    fn get_options_sql(&self) -> MbtResult<(String, Vec<u8>)> {
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
            "".to_string()
        };

        Ok((sql, query_args))
    }
}

async fn open_and_detect_type(mbtiles: &Mbtiles) -> MbtResult<MbtType> {
    let opt = SqliteConnectOptions::new()
        .read_only(true)
        .filename(mbtiles.filepath());
    let mut conn = SqliteConnection::connect_with(&opt).await?;
    mbtiles.detect_type(&mut conn).await
}

// TODO: allow for applying to different types
pub async fn apply_mbtiles_diff(
    src_file: PathBuf,
    diff_file: PathBuf,
) -> MbtResult<SqliteConnection> {
    let src_mbtiles = Mbtiles::new(src_file)?;
    let diff_mbtiles = Mbtiles::new(diff_file)?;

    let opt = SqliteConnectOptions::new().filename(src_mbtiles.filepath());
    let mut conn = SqliteConnection::connect_with(&opt).await?;
    let src_type = src_mbtiles.detect_type(&mut conn).await?;

    if src_type != Flat {
        return Err(MbtError::IncorrectDataFormat(
            src_mbtiles.filepath().to_string(),
            Flat,
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
        dst_mbttype: Option<MbtType>,
        expected_dst_mbttype: MbtType,
    ) {
        let mut dst_conn = copy_mbtiles_file(
            TileCopierOptions::new(src_filepath.clone(), dst_filepath.clone())
                .dst_mbttype(dst_mbttype),
        )
        .await
        .unwrap();

        query("ATTACH DATABASE ? AS srcDb")
            .bind(src_filepath.clone().to_str().unwrap())
            .execute(&mut dst_conn)
            .await
            .unwrap();

        assert_eq!(
            open_and_detect_type(&Mbtiles::new(dst_filepath).unwrap())
                .await
                .unwrap(),
            expected_dst_mbttype
        );

        assert!(
            query("SELECT * FROM srcDb.tiles EXCEPT SELECT * FROM tiles")
                .fetch_optional(&mut dst_conn)
                .await
                .unwrap()
                .is_none()
        )
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
    async fn copy_flat_tables() {
        let src = PathBuf::from("../tests/fixtures/files/world_cities.mbtiles");
        let dst = PathBuf::from("file:copy_flat_tables_mem_db?mode=memory&cache=shared");
        verify_copy_all(src, dst, None, Flat).await;
    }

    #[actix_rt::test]
    async fn copy_flat_from_flat_hashed_tables() {
        let src = PathBuf::from("../tests/fixtures/files/zoomed_world_cities.mbtiles");
        let dst =
            PathBuf::from("file:copy_flat_from_flat_hashed_tables_mem_db?mode=memory&cache=shared");
        verify_copy_all(src, dst, Some(Flat), Flat).await;
    }

    #[actix_rt::test]
    async fn copy_flat_from_normalized_tables() {
        let src = PathBuf::from("../tests/fixtures/files/geography-class-png.mbtiles");
        let dst =
            PathBuf::from("file:copy_flat_from_normalized_tables_mem_db?mode=memory&cache=shared");
        verify_copy_all(src, dst, Some(Flat), Flat).await;
    }

    #[actix_rt::test]
    async fn copy_flat_hashed_tables() {
        let src = PathBuf::from("../tests/fixtures/files/zoomed_world_cities.mbtiles");
        let dst = PathBuf::from("file:copy_flat_hashed_tables_mem_db?mode=memory&cache=shared");
        verify_copy_all(src, dst, None, FlatHashed).await;
    }

    #[actix_rt::test]
    async fn copy_flat_hashed_from_flat_tables() {
        let src = PathBuf::from("../tests/fixtures/files/world_cities.mbtiles");
        let dst =
            PathBuf::from("file:copy_flat_hashed_from_flat_tables_mem_db?mode=memory&cache=shared");
        verify_copy_all(src, dst, Some(FlatHashed), FlatHashed).await;
    }

    #[actix_rt::test]
    async fn copy_flat_hashed_from_normalized_tables() {
        let src = PathBuf::from("../tests/fixtures/files/geography-class-png.mbtiles");
        let dst = PathBuf::from(
            "file:copy_flat_hashed_from_normalized_tables_mem_db?mode=memory&cache=shared",
        );
        verify_copy_all(src, dst, Some(FlatHashed), FlatHashed).await;
    }

    #[actix_rt::test]
    async fn copy_normalized_tables() {
        let src = PathBuf::from("../tests/fixtures/files/geography-class-png.mbtiles");
        let dst = PathBuf::from("file:copy_normalized_tables_mem_db?mode=memory&cache=shared");
        verify_copy_all(src, dst, None, Normalized).await;
    }

    #[actix_rt::test]
    async fn copy_normalized_from_flat_tables() {
        let src = PathBuf::from("../tests/fixtures/files/world_cities.mbtiles");
        let dst =
            PathBuf::from("file:copy_normalized_from_flat_tables_mem_db?mode=memory&cache=shared");
        verify_copy_all(src, dst, Some(Normalized), Normalized).await;
    }

    #[actix_rt::test]
    async fn copy_normalized_from_flat_hashed_tables() {
        let src = PathBuf::from("../tests/fixtures/files/zoomed_world_cities.mbtiles");
        let dst = PathBuf::from(
            "file:copy_normalized_from_flat_hashed_tables_mem_db?mode=memory&cache=shared",
        );
        verify_copy_all(src, dst, Some(Normalized), Normalized).await;
    }

    #[actix_rt::test]
    async fn copy_with_min_max_zoom() {
        let src = PathBuf::from("../tests/fixtures/files/world_cities.mbtiles");
        let dst = PathBuf::from("file:copy_with_min_max_zoom_mem_db?mode=memory&cache=shared");
        let opt = TileCopierOptions::new(src, dst)
            .min_zoom(Some(2))
            .max_zoom(Some(4));
        verify_copy_with_zoom_filter(opt, 3).await;
    }

    #[actix_rt::test]
    async fn copy_with_zoom_levels() {
        let src = PathBuf::from("../tests/fixtures/files/world_cities.mbtiles");
        let dst = PathBuf::from("file:copy_with_zoom_levels_mem_db?mode=memory&cache=shared");
        let opt = TileCopierOptions::new(src, dst)
            .min_zoom(Some(2))
            .max_zoom(Some(4))
            .zoom_levels(vec![1, 6]);
        verify_copy_with_zoom_filter(opt, 2).await;
    }

    #[actix_rt::test]
    async fn copy_with_diff_with_file() {
        let src = PathBuf::from("../tests/fixtures/files/geography-class-jpg.mbtiles");
        let dst = PathBuf::from("file:copy_with_diff_with_file_mem_db?mode=memory&cache=shared");

        let diff_file =
            PathBuf::from("../tests/fixtures/files/geography-class-jpg-modified.mbtiles");

        let copy_opts =
            TileCopierOptions::new(src.clone(), dst.clone()).diff_with_file(diff_file.clone());

        let mut dst_conn = copy_mbtiles_file(copy_opts).await.unwrap();

        assert!(query("SELECT 1 FROM sqlite_schema WHERE name='tiles';")
            .fetch_optional(&mut dst_conn)
            .await
            .unwrap()
            .is_some());

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
    async fn ignore_dst_mbttype_when_copy_to_existing() {
        let src_file = PathBuf::from("../tests/fixtures/files/world_cities_modified.mbtiles");

        // Copy the dst file to an in-memory DB
        let dst_file = PathBuf::from("../tests/fixtures/files/world_cities.mbtiles");
        let dst = PathBuf::from(
            "file:ignore_dst_mbttype_when_copy_to_existing_mem_db?mode=memory&cache=shared",
        );

        let _dst_conn = copy_mbtiles_file(TileCopierOptions::new(dst_file.clone(), dst.clone()))
            .await
            .unwrap();

        verify_copy_all(src_file, dst, Some(Normalized), Flat).await;
    }

    #[actix_rt::test]
    async fn copy_to_existing_abort_mode() {
        let src = PathBuf::from("../tests/fixtures/files/world_cities_modified.mbtiles");
        let dst = PathBuf::from("../tests/fixtures/files/world_cities.mbtiles");

        let copy_opts =
            TileCopierOptions::new(src.clone(), dst.clone()).on_duplicate(CopyDuplicateMode::Abort);

        assert!(matches!(
            copy_mbtiles_file(copy_opts).await.unwrap_err(),
            MbtError::RusqliteError(..)
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

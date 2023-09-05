extern crate core;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[cfg(feature = "cli")]
use clap::{builder::ValueParser, error::ErrorKind, Args, ValueEnum};
use sqlite_hashes::register_md5_function;
use sqlite_hashes::rusqlite::{params_from_iter, Connection as RusqliteConnection};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{query, Connection, Row, SqliteConnection};

use crate::errors::MbtResult;
use crate::mbtiles::MbtType;
use crate::mbtiles::MbtType::{Flat, FlatWithHash, Normalized};
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
    /// Output format of the destination file, ignored if the file exists. If not specified, defaults to the type of source
    #[cfg_attr(feature = "cli", arg(long, value_enum))]
    dst_mbttype: Option<MbtType>,
    /// Specify copying behaviour when tiles with duplicate (zoom_level, tile_column, tile_row) values are found
    #[cfg_attr(feature = "cli", arg(long, value_enum, default_value_t = CopyDuplicateMode::default()))]
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
    /// Skip generating a global hash for mbtiles validation. By default, if dst_mbttype is flat-with-hash or normalized, generate a global hash and store in the metadata table
    #[cfg_attr(feature = "cli", arg(long))]
    skip_agg_tiles_hash: bool,
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
            skip_agg_tiles_hash: false,
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

    pub fn skip_agg_tiles_hash(mut self, skip_global_hash: bool) -> Self {
        self.skip_agg_tiles_hash = skip_global_hash;
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

        let dst_mbttype = if is_empty {
            let dst_mbttype = self
                .options
                .dst_mbttype
                .clone()
                .unwrap_or_else(|| src_mbttype.clone());

            self.create_new_mbtiles(&mut conn, &dst_mbttype, &src_mbttype)
                .await?;

            dst_mbttype
        } else if self.options.diff_with_file.is_some() {
            return Err(MbtError::NonEmptyTargetFile(self.options.dst_file));
        } else {
            open_and_detect_type(&self.dst_mbtiles).await?
        };

        let rusqlite_conn = RusqliteConnection::open(Path::new(&self.dst_mbtiles.filepath()))?;
        register_md5_function(&rusqlite_conn)?;
        rusqlite_conn.execute(
            "ATTACH DATABASE ? AS sourceDb",
            [self.src_mbtiles.filepath()],
        )?;

        let (on_dupl, sql_cond) = self.get_on_duplicate_sql(&dst_mbttype);

        let (select_from, query_args) = {
            let select_from = if let Some(diff_file) = &self.options.diff_with_file {
                let diff_with_mbtiles = Mbtiles::new(diff_file)?;
                let diff_mbttype = open_and_detect_type(&diff_with_mbtiles).await?;

                rusqlite_conn
                    .execute("ATTACH DATABASE ? AS newDb", [diff_with_mbtiles.filepath()])?;

                self.get_select_from_with_diff(&dst_mbttype, &diff_mbttype)
            } else {
                self.get_select_from(&dst_mbttype, &src_mbttype)
            };

            let (options_sql, query_args) = self.get_options_sql()?;

            (format!("{select_from} {options_sql}"), query_args)
        };

        match dst_mbttype {
            Flat => rusqlite_conn.execute(
                &format!("INSERT {on_dupl} INTO tiles {select_from} {sql_cond}"),
                params_from_iter(query_args),
            )?,
            FlatWithHash => rusqlite_conn.execute(
                &format!("INSERT {on_dupl} INTO tiles_with_hash {select_from} {sql_cond}"),
                params_from_iter(query_args),
            )?,
            Normalized => {
                rusqlite_conn.execute(
                    &format!("INSERT {on_dupl} INTO map (zoom_level, tile_column, tile_row, tile_id) SELECT zoom_level, tile_column, tile_row, hash as tile_id FROM ({select_from} {sql_cond})"),
                    params_from_iter(&query_args),
                )?;
                rusqlite_conn.execute(
                    &format!(
                        "INSERT OR IGNORE INTO images SELECT tile_data, hash FROM ({select_from})"
                    ),
                    params_from_iter(query_args),
                )?
            }
        };

        if !self.options.skip_agg_tiles_hash
            && (dst_mbttype == FlatWithHash || dst_mbttype == Normalized)
        {
            self.dst_mbtiles.update_agg_tiles_hash(&mut conn).await?;
        }

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
                FlatWithHash => self.create_flat_with_hash_tables(&mut *conn).await?,
                Normalized => self.create_normalized_tables(&mut *conn).await?,
            };
        } else {
            // DB objects must be created in a specific order: tables, views, triggers, indexes.

            for row in query(
                "SELECT sql 
                        FROM sourceDb.sqlite_schema 
                        WHERE tbl_name IN ('metadata', 'tiles', 'map', 'images', 'tiles_with_hash') 
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

        if *dst_mbttype == Normalized {
            query(
                "CREATE VIEW tiles_with_hash AS
                          SELECT
                              map.zoom_level AS zoom_level,
                              map.tile_column AS tile_column,
                              map.tile_row AS tile_row,
                              images.tile_data AS tile_data,
                              images.tile_id AS tile_hash
                          FROM map
                          JOIN images ON images.tile_id = map.tile_id",
            )
            .execute(&mut *conn)
            .await?;
        }

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

    async fn create_flat_with_hash_tables(&self, conn: &mut SqliteConnection) -> MbtResult<()> {
        for statement in &[
            "CREATE TABLE metadata (name text NOT NULL PRIMARY KEY, value text);",
            "CREATE TABLE tiles_with_hash (zoom_level integer NOT NULL, tile_column integer NOT NULL, tile_row integer NOT NULL, tile_data blob, tile_hash text,
                PRIMARY KEY(zoom_level, tile_column, tile_row));",
            "CREATE VIEW tiles AS SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles_with_hash;"] {
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
                    FlatWithHash => ("tiles_with_hash", "tile_data"),
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

    fn get_select_from_with_diff(&self, dst_mbttype: &MbtType, diff_mbttype: &MbtType) -> String {
        let (hash_col_sql, new_tiles_with_hash) = if dst_mbttype == &Flat {
            ("", "newDb.tiles")
        } else {
            match *diff_mbttype {
                Flat => (", hex(md5(tile_data)) as hash", "newDb.tiles"),
                FlatWithHash => (", new_tiles_with_hash.tile_hash as hash", "newDb.tiles_with_hash"),
                Normalized => (", new_tiles_with_hash.hash", "(SELECT zoom_level, tile_column, tile_row, tile_data, map.tile_id AS hash
                                                                    FROM newDb.map JOIN newDb.images ON newDb.map.tile_id=newDb.images.tile_id )")
            }
        };

        format!("SELECT COALESCE(sourceDb.tiles.zoom_level, new_tiles_with_hash.zoom_level) as zoom_level,
                                     COALESCE(sourceDb.tiles.tile_column, new_tiles_with_hash.tile_column) as tile_column,
                                     COALESCE(sourceDb.tiles.tile_row, new_tiles_with_hash.tile_row) as tile_row,
                                     new_tiles_with_hash.tile_data as tile_data
                                     {hash_col_sql}
                              FROM sourceDb.tiles FULL JOIN {new_tiles_with_hash} AS new_tiles_with_hash
                                   ON sourceDb.tiles.zoom_level = new_tiles_with_hash.zoom_level
                                   AND sourceDb.tiles.tile_column = new_tiles_with_hash.tile_column
                                   AND sourceDb.tiles.tile_row = new_tiles_with_hash.tile_row
                              WHERE (sourceDb.tiles.tile_data != new_tiles_with_hash.tile_data
                                  OR sourceDb.tiles.tile_data ISNULL
                                  OR new_tiles_with_hash.tile_data ISNULL)")
    }

    fn get_select_from(&self, dst_mbttype: &MbtType, src_mbttype: &MbtType) -> String {
        let select_from = if dst_mbttype == &Flat {
            "SELECT * FROM sourceDb.tiles "
        } else {
            match *src_mbttype {
                Flat => "SELECT *, hex(md5(tile_data)) as hash FROM sourceDb.tiles ",
                FlatWithHash => "SELECT zoom_level, tile_column, tile_row, tile_data, tile_hash AS hash FROM sourceDb.tiles_with_hash",
                Normalized => "SELECT zoom_level, tile_column, tile_row, tile_data, map.tile_id AS hash FROM sourceDb.map JOIN sourceDb.images ON sourceDb.map.tile_id=sourceDb.images.tile_id"
            }
        }.to_string();

        format!("{select_from} WHERE TRUE ")
    }

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

pub async fn apply_mbtiles_diff(src_file: PathBuf, diff_file: PathBuf) -> MbtResult<()> {
    let src_mbtiles = Mbtiles::new(src_file)?;
    let diff_mbtiles = Mbtiles::new(diff_file)?;

    let src_mbttype = open_and_detect_type(&src_mbtiles).await?;
    let diff_mbttype = open_and_detect_type(&diff_mbtiles).await?;

    let rusqlite_conn = RusqliteConnection::open(Path::new(&src_mbtiles.filepath()))?;
    register_md5_function(&rusqlite_conn)?;
    rusqlite_conn.execute("ATTACH DATABASE ? AS diffDb", [diff_mbtiles.filepath()])?;

    let select_from = if src_mbttype == Flat {
        "SELECT * FROM diffDb.tiles "
    } else {
        match diff_mbttype {
            Flat => "SELECT *, hex(md5(tile_data)) as hash FROM diffDb.tiles ",
            FlatWithHash => "SELECT zoom_level, tile_column, tile_row, tile_data, tile_hash AS hash FROM diffDb.tiles_with_hash",
            Normalized => "SELECT zoom_level, tile_column, tile_row, tile_data, map.tile_id AS hash FROM diffDb.map LEFT JOIN diffDb.images ON diffDb.map.tile_id=diffDb.images.tile_id"
        }
    }.to_string();

    let (insert_sql, main_table) = match src_mbttype {
        Flat => (vec![format!(
            "INSERT OR REPLACE INTO tiles (zoom_level, tile_column, tile_row, tile_data) {select_from}"
        )], "tiles".to_string()),
        FlatWithHash => (vec![format!(
            "INSERT OR REPLACE INTO tiles_with_hash {select_from}"
        )], "tiles_with_hash".to_string()),
        Normalized => (vec![format!("INSERT OR REPLACE INTO map (zoom_level, tile_column, tile_row, tile_id) SELECT zoom_level, tile_column, tile_row, hash as tile_id FROM ({select_from})"), format!(
            "INSERT OR REPLACE INTO images SELECT tile_data, hash FROM ({select_from})"
        )], "map".to_string())
    };

    for statement in insert_sql {
        rusqlite_conn.execute(&format!("{statement} WHERE tile_data NOTNULL"), ())?;
    }

    rusqlite_conn.execute(
        &format!(
            "DELETE FROM {main_table} WHERE (zoom_level, tile_column, tile_row) IN (SELECT zoom_level, tile_column, tile_row FROM ({select_from} WHERE tile_data ISNULL));"
        ),()
    )?;

    Ok(())
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
    async fn copy_flat_from_flat_with_hash_tables() {
        let src = PathBuf::from("../tests/fixtures/files/zoomed_world_cities.mbtiles");
        let dst = PathBuf::from(
            "file:copy_flat_from_flat_with_hash_tables_mem_db?mode=memory&cache=shared",
        );
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
    async fn copy_flat_with_hash_tables() {
        let src = PathBuf::from("../tests/fixtures/files/zoomed_world_cities.mbtiles");
        let dst = PathBuf::from("file:copy_flat_with_hash_tables_mem_db?mode=memory&cache=shared");
        verify_copy_all(src, dst, None, FlatWithHash).await;
    }

    #[actix_rt::test]
    async fn copy_flat_with_hash_from_flat_tables() {
        let src = PathBuf::from("../tests/fixtures/files/world_cities.mbtiles");
        let dst = PathBuf::from(
            "file:copy_flat_with_hash_from_flat_tables_mem_db?mode=memory&cache=shared",
        );
        verify_copy_all(src, dst, Some(FlatWithHash), FlatWithHash).await;
    }

    #[actix_rt::test]
    async fn copy_flat_with_hash_from_normalized_tables() {
        let src = PathBuf::from("../tests/fixtures/files/geography-class-png.mbtiles");
        let dst = PathBuf::from(
            "file:copy_flat_with_hash_from_normalized_tables_mem_db?mode=memory&cache=shared",
        );
        verify_copy_all(src, dst, Some(FlatWithHash), FlatWithHash).await;
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
    async fn copy_normalized_from_flat_with_hash_tables() {
        let src = PathBuf::from("../tests/fixtures/files/zoomed_world_cities.mbtiles");
        let dst = PathBuf::from(
            "file:copy_normalized_from_flat_with_hash_tables_mem_db?mode=memory&cache=shared",
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
            get_one::<i32>(&mut dst_conn, "SELECT COUNT(*) FROM map;").await,
            3
        );

        assert!(get_one::<Option<i32>>(
            &mut dst_conn,
            "SELECT * FROM tiles WHERE zoom_level=2 AND tile_row=2 AND tile_column=2;"
        )
        .await
        .is_some());

        assert!(get_one::<Option<i32>>(
            &mut dst_conn,
            "SELECT * FROM tiles WHERE zoom_level=1 AND tile_row=1 AND tile_column=1;"
        )
        .await
        .is_some());

        assert!(get_one::<Option<i32>>(
            &mut dst_conn,
            "SELECT tile_id FROM map WHERE zoom_level=0 AND tile_row=0 AND tile_column=0;"
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
    async fn apply_flat_diff_file() {
        // Copy the src file to an in-memory DB
        let src_file = PathBuf::from("../tests/fixtures/files/world_cities.mbtiles");
        let src = PathBuf::from("file:apply_flat_diff_file_mem_db?mode=memory&cache=shared");

        let mut src_conn = copy_mbtiles_file(TileCopierOptions::new(src_file.clone(), src.clone()))
            .await
            .unwrap();

        // Apply diff to the src data in in-memory DB
        let diff_file = PathBuf::from("../tests/fixtures/files/world_cities_diff.mbtiles");
        apply_mbtiles_diff(src, diff_file).await.unwrap();

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

    #[actix_rt::test]
    async fn apply_normalized_diff_file() {
        // Copy the src file to an in-memory DB
        let src_file = PathBuf::from("../tests/fixtures/files/geography-class-jpg.mbtiles");
        let src = PathBuf::from("file:apply_normalized_diff_file_mem_db?mode=memory&cache=shared");

        let mut src_conn = copy_mbtiles_file(TileCopierOptions::new(src_file.clone(), src.clone()))
            .await
            .unwrap();

        // Apply diff to the src data in in-memory DB
        let diff_file = PathBuf::from("../tests/fixtures/files/geography-class-jpg-diff.mbtiles");
        apply_mbtiles_diff(src, diff_file).await.unwrap();

        // Verify the data is the same as the file the diff was generated from
        let path = "../tests/fixtures/files/geography-class-jpg-modified.mbtiles";
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

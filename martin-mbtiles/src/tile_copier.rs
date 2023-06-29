extern crate core;

use std::collections::HashSet;
use std::path::PathBuf;

#[cfg(feature = "cli")]
use clap::{builder::ValueParser, error::ErrorKind, Args};
use sqlx::sqlite::{SqliteArguments, SqliteConnectOptions};
use sqlx::{query, query_with, Arguments, Connection, Row, SqliteConnection};

use crate::errors::MbtResult;
use crate::mbtiles::MbtType;
use crate::{MbtError, Mbtiles};

#[derive(Clone, Default, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "cli", derive(Args))]
pub struct TileCopierOptions {
    /// MBTiles file to read from
    src_file: PathBuf,
    /// MBTiles file to write to
    dst_file: PathBuf,
    /// Force the output file to be in a simple MBTiles format with a `tiles` table
    #[cfg_attr(feature = "cli", arg(long))]
    force_simple: bool,
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
    options: TileCopierOptions,
}

impl TileCopierOptions {
    pub fn new(src_filepath: PathBuf, dst_filepath: PathBuf) -> Self {
        Self {
            src_file: src_filepath,
            dst_file: dst_filepath,
            zoom_levels: HashSet::new(),
            force_simple: false,
            min_zoom: None,
            max_zoom: None,
            diff_with_file: None,
        }
    }

    pub fn force_simple(mut self, force_simple: bool) -> Self {
        self.force_simple = force_simple;
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
            options,
        })
    }

    pub async fn run(self) -> MbtResult<SqliteConnection> {
        let storage_type = self.detect_type(&self.src_mbtiles).await?;
        let force_simple = self.options.force_simple && storage_type != MbtType::TileTables;

        let opt = SqliteConnectOptions::new()
            .create_if_missing(true)
            .filename(&self.options.dst_file);
        let mut conn = SqliteConnection::connect_with(&opt).await?;

        if query("SELECT 1 FROM sqlite_schema LIMIT 1")
            .fetch_optional(&mut conn)
            .await?
            .is_some()
        {
            return Err(MbtError::NonEmptyTargetFile(self.options.dst_file));
        }

        query("PRAGMA page_size = 512").execute(&mut conn).await?;
        query("VACUUM").execute(&mut conn).await?;

        query("ATTACH DATABASE ? AS sourceDb")
            .bind(self.src_mbtiles.filepath())
            .execute(&mut conn)
            .await?;

        if force_simple {
            for statement in &["CREATE TABLE metadata (name text, value text);",
                "CREATE TABLE tiles (zoom_level integer, tile_column integer, tile_row integer, tile_data blob);",
                "CREATE UNIQUE INDEX name on metadata (name);",
                "CREATE UNIQUE INDEX tile_index on tiles (zoom_level, tile_column, tile_row);"] {
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

        if force_simple {
            self.copy_tile_tables(&mut conn).await?
        } else {
            match storage_type {
                MbtType::TileTables => self.copy_tile_tables(&mut conn).await?,
                MbtType::DeDuplicated => self.copy_deduplicated(&mut conn).await?,
            }
        }

        Ok(conn)
    }

    async fn copy_tile_tables(&self, conn: &mut SqliteConnection) -> MbtResult<()> {
        if let Some(diff_file) = &self.options.diff_with_file {
            let diff_mbtiles = Mbtiles::new(diff_file)?;

            let _ = &self.detect_type(&diff_mbtiles);

            query("ATTACH DATABASE ? AS newDb")
                .bind(diff_mbtiles.filepath())
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
                "INSERT INTO tiles SELECT * FROM sourceDb.tiles WHERE TRUE",
            )
            .await
        }
    }

    async fn copy_deduplicated(&self, conn: &mut SqliteConnection) -> MbtResult<()> {
        query("INSERT INTO map SELECT * FROM sourceDb.map")
            .execute(&mut *conn)
            .await?;

        self.run_query_with_options(
            conn,
            // Allows for adding clauses to query using "AND"
            "INSERT INTO images
                SELECT images.tile_data, images.tile_id
                FROM sourceDb.images
                  JOIN sourceDb.map
                  ON images.tile_id = map.tile_id
                WHERE TRUE",
        )
        .await
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

    async fn detect_type(&self, mbtiles: &Mbtiles) -> MbtResult<MbtType> {
        let opt = SqliteConnectOptions::new()
            .read_only(true)
            .filename(mbtiles.filepath());
        let mut conn = SqliteConnection::connect_with(&opt).await?;
        mbtiles.detect_type(&mut conn).await
    }
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
    async fn non_empty_target_file() {
        let src = PathBuf::from("../tests/fixtures/files/world_cities.mbtiles");
        let dst = PathBuf::from("../tests/fixtures/files/json.mbtiles");
        assert!(matches!(
            TileCopier::new(TileCopierOptions::new(src, dst))
                .unwrap()
                .run()
                .await,
            Err(MbtError::NonEmptyTargetFile(_))
        ));
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
                "SELECT tile_data FROM tiles where zoom_level=2 AND tile_row=2 AND tile_column=2;"
            )
            .await,
            2
        );

        assert_eq!(
            get_one::<String>(
                &mut dst_conn,
                "SELECT tile_data FROM tiles where zoom_level=1 AND tile_row=1 AND tile_column=1;"
            )
            .await,
            "4"
        );

        assert!(get_one::<Option<i32>>(
            &mut dst_conn,
            "SELECT tile_data FROM tiles where zoom_level=0 AND tile_row=0 AND tile_column=0;"
        )
        .await
        .is_none());
    }
}

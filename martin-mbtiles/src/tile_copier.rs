extern crate core;

use crate::errors::MbtResult;
use crate::mbtiles::MbtType;
use crate::{MbtError, Mbtiles};
use sqlx::sqlite::{SqliteArguments, SqliteConnectOptions};
use sqlx::{query, query_with, Arguments, Connection, Row, SqliteConnection};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Clone, Default, Debug)]
pub struct TileCopierOptions {
    zooms: HashSet<u8>,
    force_simple: bool,
    min_zoom: Option<u8>,
    max_zoom: Option<u8>,
    verbose: bool,
}

#[derive(Clone, Debug)]
pub struct TileCopier {
    src_mbtiles: Mbtiles,
    dst_filepath: PathBuf,
    options: TileCopierOptions,
}

impl TileCopierOptions {
    pub fn new() -> Self {
        Self {
            zooms: HashSet::new(),
            force_simple: false,
            min_zoom: None,
            max_zoom: None,
            verbose: false,
        }
    }

    pub fn force_simple(mut self, force_simple: bool) -> Self {
        self.force_simple = force_simple;
        self
    }

    pub fn zooms(mut self, zooms: Vec<u8>) -> Self {
        for zoom in zooms {
            self.zooms.insert(zoom);
        }
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

    pub fn verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }
}

impl TileCopier {
    pub fn new(
        src_filepath: PathBuf,
        dst_filepath: PathBuf,
        options: TileCopierOptions,
    ) -> MbtResult<Self> {
        Ok(TileCopier {
            src_mbtiles: Mbtiles::new(src_filepath)?,
            dst_filepath,
            options,
        })
    }

    pub async fn run(self) -> MbtResult<SqliteConnection> {
        let opt = SqliteConnectOptions::new()
            .create_if_missing(true)
            .filename(&self.dst_filepath);
        let mut conn = SqliteConnection::connect_with(&opt).await?;

        if query("SELECT 1 FROM sqlite_schema LIMIT 1")
            .fetch_optional(&mut conn)
            .await?
            .is_some()
        {
            return Err(MbtError::NonEmptyTargetFile(self.dst_filepath));
        }

        query("PRAGMA page_size = 512").execute(&mut conn).await?;
        query("VACUUM").execute(&mut conn).await?;

        query("ATTACH DATABASE ? AS sourceDb")
            .bind(self.src_mbtiles.filepath())
            .execute(&mut conn)
            .await?;

        if !self.options.force_simple {
            for row in query("SELECT sql FROM sourceDb.sqlite_schema WHERE tbl_name IN ('metadata', 'tiles', 'map', 'images')")
                .fetch_all(&mut conn)
                .await? {
                query(row.get(0)).execute(&mut conn).await?;
            }
        } else {
            for statement in &["CREATE TABLE metadata (name text, value text);",
                "CREATE TABLE tiles (zoom_level integer, tile_column integer, tile_row integer, tile_data blob);",
                "CREATE UNIQUE INDEX name on metadata (name);",
                "CREATE UNIQUE INDEX tile_index on tiles (zoom_level, tile_column, tile_row);"] {
                query(statement).execute(&mut conn).await?;
            }
        };

        query("INSERT INTO metadata SELECT * FROM sourceDb.metadata")
            .execute(&mut conn)
            .await?;

        if !self.options.force_simple {
            let src_opt = SqliteConnectOptions::new()
                .read_only(true)
                .filename(PathBuf::from(&self.src_mbtiles.filepath()));
            let mut src_conn = SqliteConnection::connect_with(&src_opt).await?;

            match self.src_mbtiles.detect_type(&mut src_conn).await? {
                MbtType::TileTables => self.copy_tile_tables(&mut conn).await?,
                MbtType::DeDuplicated => self.copy_deduplicated(&mut conn).await?,
            }
        } else {
            self.copy_tile_tables(&mut conn).await?
        }

        Ok(conn)
    }

    async fn copy_tile_tables(&self, conn: &mut SqliteConnection) -> MbtResult<()> {
        self.run_query_with_options(conn, "INSERT INTO tiles SELECT * FROM sourceDb.tiles")
            .await
    }

    async fn copy_deduplicated(&self, conn: &mut SqliteConnection) -> MbtResult<()> {
        query("INSERT INTO map SELECT * FROM sourceDb.map")
            .execute(&mut *conn)
            .await?;

        self.run_query_with_options(
            conn,
            "INSERT INTO images
                SELECT images.tile_data, images.tile_id
                FROM sourceDb.images
                  JOIN sourceDb.map
                  ON images.tile_id = map.tile_id",
        )
        .await
    }

    async fn run_query_with_options(
        &self,
        conn: &mut SqliteConnection,
        sql: &str,
    ) -> MbtResult<()> {
        let mut params = SqliteArguments::default();

        let sql = if !&self.options.zooms.is_empty() {
            for z in &self.options.zooms {
                params.add(z);
            }
            format!(
                "{sql} WHERE zoom_level IN ({})",
                vec!["?"; self.options.zooms.len()].join(",")
            )
        } else if let Some(min_zoom) = &self.options.min_zoom {
            if let Some(max_zoom) = &self.options.max_zoom {
                params.add(min_zoom);
                params.add(max_zoom);
                format!("{sql} WHERE zoom_level BETWEEN ? AND ?")
            } else {
                params.add(min_zoom);
                format!("{sql} WHERE zoom_level >= ?")
            }
        } else if let Some(max_zoom) = &self.options.max_zoom {
            params.add(max_zoom);
            format!("{sql} WHERE zoom_level <= ?")
        } else {
            sql.to_string()
        };

        query_with(sql.as_str(), params).execute(conn).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sqlx::{Connection, SqliteConnection};

    use super::*;

    async fn open_sql(path: &PathBuf) -> SqliteConnection {
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(path))
            .await
            .unwrap()
    }

    async fn verify_copy_all(src_filepath: PathBuf, dst_filepath: PathBuf) {
        let mut dst_conn = TileCopier::new(
            src_filepath.clone(),
            dst_filepath.clone(),
            TileCopierOptions::new(),
        )
        .unwrap()
        .run()
        .await
        .unwrap();

        assert_eq!(
            query("SELECT COUNT(*) FROM tiles;")
                .fetch_one(&mut open_sql(&src_filepath).await)
                .await
                .unwrap()
                .get::<i32, _>(0),
            query("SELECT COUNT(*) FROM tiles;")
                .fetch_one(&mut dst_conn)
                .await
                .unwrap()
                .get::<i32, _>(0)
        );
    }

    async fn verify_copy_with_zoom_filter(
        src_filepath: PathBuf,
        dst_filepath: PathBuf,
        opts: TileCopierOptions,
        expected_zoom_levels: u8,
    ) {
        let mut dst_conn = TileCopier::new(src_filepath, dst_filepath.clone(), opts)
            .unwrap()
            .run()
            .await
            .unwrap();

        assert_eq!(
            query("SELECT COUNT(DISTINCT zoom_level) FROM tiles;")
                .fetch_one(&mut dst_conn)
                .await
                .unwrap()
                .get::<u8, _>(0),
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
            TileCopier::new(src, dst, TileCopierOptions::new())
                .unwrap()
                .run()
                .await,
            Err(MbtError::NonEmptyTargetFile(_))
        ));
    }

    #[actix_rt::test]
    async fn copy_force_simple() {
        let src_filepath = PathBuf::from("../tests/fixtures/files/world_cities.mbtiles");
        let dst_filepath = PathBuf::from(":memory:");

        let copy_opts = TileCopierOptions::new().force_simple(true);

        let mut dst_conn = TileCopier::new(src_filepath.clone(), dst_filepath.clone(), copy_opts)
            .unwrap()
            .run()
            .await
            .unwrap();

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
        let opt = TileCopierOptions::new().min_zoom(Some(2)).max_zoom(Some(4));
        verify_copy_with_zoom_filter(src, dst, opt, 3).await;
    }

    #[actix_rt::test]
    async fn copy_with_zoom_levels() {
        let src = PathBuf::from("../tests/fixtures/files/world_cities.mbtiles");
        let dst = PathBuf::from(":memory:");
        let opt = TileCopierOptions::new()
            .min_zoom(Some(2))
            .max_zoom(Some(4))
            .zooms(vec![1, 6]);
        verify_copy_with_zoom_filter(src, dst, opt, 2).await;
    }
}

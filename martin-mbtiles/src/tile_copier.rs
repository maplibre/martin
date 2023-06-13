extern crate core;

use crate::errors::MbtResult;
use crate::mbtiles::Type;
use crate::{MbtError, Mbtiles};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{query, Connection, Row, SqliteConnection};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct TileCopier {
    src_mbtiles: Mbtiles,
    dst_filepath: PathBuf,
    zooms: HashSet<u8>,
    min_zoom: Option<u8>,
    max_zoom: Option<u8>,
    //self.bbox = bbox
    verbose: bool,
}

impl TileCopier {
    pub fn new(src_filepath: PathBuf, dst_filepath: PathBuf) -> MbtResult<Self> {
        Ok(TileCopier {
            src_mbtiles: Mbtiles::new(src_filepath)?,
            dst_filepath,
            zooms: HashSet::new(),
            min_zoom: None,
            max_zoom: None,
            verbose: false,
        })
    }

    pub fn zooms(&mut self, zooms: Vec<u8>) -> &mut Self {
        for zoom in zooms {
            self.zooms.insert(zoom);
        }
        self
    }

    pub fn min_zoom(&mut self, min_zoom: u8) -> &mut Self {
        self.min_zoom = Some(min_zoom);
        self
    }

    pub fn max_zoom(&mut self, max_zoom: u8) -> &mut Self {
        self.max_zoom = Some(max_zoom);
        self
    }

    pub fn verbose(&mut self, verbose: bool) -> &mut Self {
        self.verbose = verbose;
        self
    }

    pub async fn run(self) -> MbtResult<()> {
        let opt = SqliteConnectOptions::new()
            .read_only(true)
            .filename(PathBuf::from(&self.src_mbtiles.filepath()));
        let mut conn = SqliteConnection::connect_with(&opt).await?;
        let storage_type = self.src_mbtiles.detect_type(&mut conn).await?;

        let opt = SqliteConnectOptions::new()
            .create_if_missing(true)
            .filename(&self.dst_filepath);
        let mut conn = SqliteConnection::connect_with(&opt).await?;

        if query("SELECT 1 FROM sqlite_schema")
            .fetch_optional(&mut conn)
            .await?
            .is_some()
        {
            return Err(MbtError::NonEmptyTargetFile(self.dst_filepath.clone()));
        }

        query("PRAGMA page_size = 512").execute(&mut conn).await?;
        query("VACUUM").execute(&mut conn).await?;

        query("ATTACH DATABASE ? AS sourceDb")
            .bind(self.src_mbtiles.filepath())
            .execute(&mut conn)
            .await?;

        let schema = query("SELECT sql FROM sourceDb.sqlite_schema WHERE tbl_name IN ('metadata', 'tiles', 'map', 'images')")
            .fetch_all(&mut conn)
            .await?;

        for row in &schema {
            let row: String = row.get(0);
            query(row.as_str()).execute(&mut conn).await?;
        }

        query("INSERT INTO metadata SELECT * FROM sourceDb.metadata")
            .execute(&mut conn)
            .await?;

        match storage_type {
            Type::TileTables => self.copy_standard_compliant_tiles(&mut conn).await,
            Type::DeDuplicated => self.copy_deduplicated_tiles(&mut conn).await,
        }
    }

    async fn copy_standard_compliant_tiles(&self, conn: &mut SqliteConnection) -> MbtResult<()> {
        // TODO: Handle options
        //  - bbox
        //  - verbose
        //  - zoom

        self.run_conditional_query(
            conn,
            String::from("INSERT INTO tiles SELECT * FROM sourceDb.tiles"),
        )
        .await?;

        Ok(())
    }

    async fn copy_deduplicated_tiles(&self, conn: &mut SqliteConnection) -> MbtResult<()> {
        query("INSERT INTO map SELECT * FROM sourceDb.map")
            .execute(&mut *conn)
            .await?;

        query("INSERT INTO images SELECT * FROM sourceDb.images")
            .execute(conn)
            .await?;

        Ok(())
    }

    // TODO:: rename this
    async fn run_conditional_query(
        &self,
        conn: &mut SqliteConnection,
        mut sql: String,
    ) -> MbtResult<()> {
        let mut params: Vec<String> = vec![];

        if !&self.zooms.is_empty() {
            sql.push_str(
                format!(
                    " WHERE zoom_level IN ({})",
                    vec!["?"; self.zooms.len()].join(",")
                )
                .as_str(),
            );
            for zoom_level in &self.zooms {
                params.push(zoom_level.to_string());
            }
        } else if let Some(min_zoom) = &self.min_zoom {
            if let Some(max_zoom) = &self.max_zoom {
                sql.push_str(" WHERE zoom_level BETWEEN ? AND ?");

                params.push(min_zoom.to_string());
                params.push(max_zoom.to_string());
            } else {
                sql.push_str(" WHERE zoom_level >= ? ");

                params.push(min_zoom.to_string());
            }
        } else if let Some(max_zoom) = &self.max_zoom {
            sql.push_str(" WHERE zoom_level <= ? ");

            params.push(max_zoom.to_string());
        }

        let mut query = query(sql.as_str());

        for param in params {
            query = query.bind(param);
        }

        query.execute(conn).await?;

        Ok(())
    }
}

// TODO: tests
// TODO: documentation

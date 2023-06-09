extern crate core;

use crate::errors::MbtResult;
use crate::MbtError;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{query, Connection, Row, SqliteConnection, SqliteExecutor};
use std::fs::File;
use std::path::PathBuf;

#[derive(Debug)]
enum StorageType {
    Unknown,
    StandardCompliant,
    Deduplicated,
}

#[derive(Clone, Default, Debug)]
pub struct TileCopier {
    src_filepath: PathBuf,
    dst_filepath: PathBuf,
    zooms: Vec<u8>,
    min_zoom: Option<u8>,
    max_zoom: Option<u8>,
    //self.bbox = bbox
    verbose: bool,
}

impl TileCopier {
    pub fn new(src_filepath: PathBuf, dst_filepath: PathBuf) -> TileCopier {
        TileCopier {
            src_filepath,
            dst_filepath,
            zooms: Vec::new(),
            min_zoom: None,
            max_zoom: None,
            verbose: false,
        }
    }

    pub fn zooms(&mut self, zooms: &[u8]) -> &mut Self {
        self.zooms.extend_from_slice(zooms);
        self
    }

    pub fn min_zoom(&mut self, min_zoom: u8) -> &mut Self {
        self.min_zoom = Some(min_zoom);
        self
    }

    pub fn max_zoom(&mut self, max_zoom: u8) -> &mut Self {
        self.min_zoom = Some(max_zoom);
        self
    }

    pub fn verbose(&mut self, verbose: bool) -> &mut Self {
        self.verbose = verbose;
        self
    }

    pub async fn run(&self) -> MbtResult<()> {
        let opt = SqliteConnectOptions::new()
            .filename(&self.src_filepath)
            .read_only(true);
        let mut conn = SqliteConnection::connect_with(&opt).await?;
        self.copy_tiles(&mut conn).await
    }

    async fn copy_tiles<T>(&self, src_conn: &mut T) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let storage_type = self.determine_storage_type(src_conn).await?;

        return match storage_type {
            Some(StorageType::StandardCompliant) => {
                self.copy_standard_compliant_tiles(src_conn).await
            }
            Some(StorageType::Deduplicated) => self.copy_deduplicated_tiles(src_conn).await,
            _ => Err(MbtError::InvalidDataStorageFormat(
                self.src_filepath.clone(),
            )),
        };
    }

    async fn determine_storage_type<T>(&self, conn: &mut T) -> MbtResult<Option<StorageType>>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        if let Some(_v) = query(include_str!("queries/is_standard_compliant_mbtiles.sql"))
            .fetch_optional(&mut *conn)
            .await?
        {
            return Ok(Some(StorageType::StandardCompliant));
        } else if let Some(_v) = query(include_str!("queries/is_deduplicated_mbtiles.sql"))
            .fetch_optional(&mut *conn)
            .await?
        {
            return Ok(Some(StorageType::Deduplicated));
        }

        Ok(Some(StorageType::Unknown))
    }

    async fn create_target_db<T>(&self, conn: &mut T) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        match File::create(&self.dst_filepath) {
            Ok(_) => {}
            Err(_) => return Err(MbtError::CouldNotCreateMBTiles(self.dst_filepath.clone())),
        };
        let opt = SqliteConnectOptions::new().filename(&self.dst_filepath);
        let mut dst_conn = SqliteConnection::connect_with(&opt).await?;
        //TODO: Q fix this with format string?
        let schema = query("SELECT sql FROM sqlite_schema WHERE tbl_name IN ('metadata', 'tiles', 'map', 'images')")
            .fetch_all(conn)
            .await?;

        query("PRAGMA page_size = 512").execute(&mut dst_conn);
        query("VACUUM").execute(&mut dst_conn);

        for row in schema.iter() {
            let row: String = row.get(0);
            query(row.as_str()).execute(&mut dst_conn).await?;
        }

        dst_conn.close();

        Ok(())
    }

    async fn copy_standard_compliant_tiles<T>(&self, conn: &mut T) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        // TODO: Handle options
        //  - bbox
        //  - verbose
        //  - zoom
        self.create_target_db(conn).await?;

        let opt = SqliteConnectOptions::new().filename(&self.dst_filepath);
        let mut dst_conn = SqliteConnection::connect_with(&opt).await?;

        query(&*format!(
            "ATTACH DATABASE '{}' AS sourceDb",
            match self.src_filepath.to_str() {
                Some(v) => v,
                None => return Err(MbtError::NoSuchMBTiles(self.src_filepath.clone())),
            }
        )) // TODO: Q can this be done without the format string?
        .execute(&mut dst_conn)
        .await?;

        query("INSERT INTO metadata SELECT * FROM sourceDb.metadata")
            .execute(&mut dst_conn)
            .await?;

        query("INSERT INTO tiles SELECT * FROM sourceDb.tiles")
            .execute(&mut dst_conn)
            .await?;

        Ok(())
    }

    async fn copy_deduplicated_tiles<T>(&self, conn: &mut T) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        self.create_target_db(conn).await?;

        let opt = SqliteConnectOptions::new().filename(&self.dst_filepath);
        let mut dst_conn = SqliteConnection::connect_with(&opt).await?;

        query(&*format!(
            "ATTACH DATABASE '{}' AS sourceDb",
            match self.src_filepath.to_str() {
                Some(v) => v,
                None => return Err(MbtError::NoSuchMBTiles(self.src_filepath.clone())),
            }
        ))
        .execute(&mut dst_conn)
        .await?;

        query("INSERT INTO metadata SELECT * FROM sourceDb.metadata")
            .execute(&mut dst_conn)
            .await?;

        query("INSERT INTO map SELECT * FROM sourceDb.map")
            .execute(&mut dst_conn)
            .await?;

        query("INSERT INTO images SELECT * FROM sourceDb.images")
            .execute(&mut dst_conn)
            .await?;

        Ok(())
    }
}

// TODO: tests
// TODO: documentation

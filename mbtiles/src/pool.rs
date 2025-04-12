use std::path::Path;

use sqlx::{Pool, Sqlite, SqlitePool};

use crate::errors::MbtResult;
use crate::{Mbtiles, Metadata};

#[derive(Clone, Debug)]
pub struct MbtilesPool {
    mbtiles: Mbtiles,
    pool: Pool<Sqlite>,
}

impl MbtilesPool {
    pub async fn new<P: AsRef<Path>>(filepath: P) -> MbtResult<Self> {
        let mbtiles = Mbtiles::new(filepath)?;
        let pool = SqlitePool::connect(mbtiles.filepath()).await?;
        Ok(Self { mbtiles, pool })
    }

    /// Get the metadata of the MBTiles file.
    ///
    /// See [`Metadata`] for more information.
    pub async fn get_metadata(&self) -> MbtResult<Metadata> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles.get_metadata(&mut *conn).await
    }

    /// Get a tile from the pool
    pub async fn get_tile(&self, z: u8, x: u32, y: u32) -> MbtResult<Option<Vec<u8>>> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles.get_tile(&mut *conn, z, x, y).await
    }
}

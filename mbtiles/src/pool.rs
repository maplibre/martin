use std::path::Path;

use sqlx::{Pool, Sqlite, SqlitePool};

use crate::errors::MbtResult;
use crate::mbtiles::ValidationLevel;
use crate::{IntegrityCheckType, Mbtiles, Metadata};

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

    pub async fn get_metadata(&self) -> MbtResult<Metadata> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles.get_metadata(&mut *conn).await
    }

    pub async fn get_tile(&self, z: u8, x: u32, y: u32) -> MbtResult<Option<Vec<u8>>> {
        let mut conn = self.pool.acquire().await?;
        self.mbtiles.get_tile(&mut *conn, z, x, y).await
    }

    pub async fn validate(&self, validation_level: ValidationLevel) -> MbtResult<()> {
        let mut conn = self.pool.acquire().await?;
        match validation_level {
            ValidationLevel::Thorough => {
                self.mbtiles.detect_type(&mut *conn).await?;
                self.mbtiles.check_integrity(&mut *conn, IntegrityCheckType::Full).await?;
                self.mbtiles.check_tiles_type_validity(&mut *conn).await?;
                self.mbtiles.check_each_tile_hash(&mut *conn).await?;
            }
            ValidationLevel::Fast => {
                self.mbtiles.detect_type(&mut *conn).await?;
            }
            ValidationLevel::Skip => {}
        }
        Ok(())
    }
}

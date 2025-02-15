use std::path::Path;

use sqlx::{Pool, Sqlite, SqlitePool};

use crate::errors::MbtResult;
use crate::mbtiles::ValidationLevel;
use crate::{AggHashType, IntegrityCheckType, Mbtiles, Metadata};

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
                self.mbtiles.validate(&mut *conn, IntegrityCheckType::Full, AggHashType::Verify).await?;
            }
            ValidationLevel::Fast => {
                self.mbtiles.detect_type(&mut *conn).await?;
            }
            ValidationLevel::Skip => {}
        }
        Ok(())
    }
}

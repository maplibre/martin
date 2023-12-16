use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::path::Path;

use enum_display::EnumDisplay;
use log::debug;
use serde::{Deserialize, Serialize};
use sqlite_hashes::register_md5_function;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{query, Connection as _, Executor, SqliteConnection, SqliteExecutor, Statement};

use crate::errors::{MbtError, MbtResult};
use crate::{invert_y_value, CopyDuplicateMode, MbtType};

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize, EnumDisplay)]
#[enum_display(case = "Kebab")]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum MbtTypeCli {
    Flat,
    FlatWithHash,
    Normalized,
}

#[derive(Clone, Debug)]
pub struct Mbtiles {
    filepath: String,
    filename: String,
}

impl Display for Mbtiles {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.filepath)
    }
}

impl Mbtiles {
    pub fn new<P: AsRef<Path>>(filepath: P) -> MbtResult<Self> {
        let path = filepath.as_ref();
        Ok(Self {
            filepath: path
                .to_str()
                .ok_or_else(|| MbtError::UnsupportedCharsInFilepath(path.to_path_buf()))?
                .to_string(),
            filename: path
                .file_stem()
                .unwrap_or_else(|| OsStr::new("unknown"))
                .to_string_lossy()
                .to_string(),
        })
    }

    pub async fn open(&self) -> MbtResult<SqliteConnection> {
        debug!("Opening w/ defaults {self}");
        let opt = SqliteConnectOptions::new().filename(self.filepath());
        Self::open_int(&opt).await
    }

    pub async fn open_or_new(&self) -> MbtResult<SqliteConnection> {
        debug!("Opening or creating {self}");
        let opt = SqliteConnectOptions::new()
            .filename(self.filepath())
            .create_if_missing(true);
        Self::open_int(&opt).await
    }

    pub async fn open_readonly(&self) -> MbtResult<SqliteConnection> {
        debug!("Opening as readonly {self}");
        let opt = SqliteConnectOptions::new()
            .filename(self.filepath())
            .read_only(true);
        Self::open_int(&opt).await
    }

    async fn open_int(opt: &SqliteConnectOptions) -> Result<SqliteConnection, MbtError> {
        let mut conn = SqliteConnection::connect_with(opt).await?;
        attach_hash_fn(&mut conn).await?;
        Ok(conn)
    }

    #[must_use]
    pub fn filepath(&self) -> &str {
        &self.filepath
    }

    #[must_use]
    pub fn filename(&self) -> &str {
        &self.filename
    }

    /// Attach this `MBTiles` file to the given `SQLite` connection as a given name
    pub async fn attach_to<T>(&self, conn: &mut T, name: &str) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        debug!("Attaching {self} as {name}");
        query(&format!("ATTACH DATABASE ? AS {name}"))
            .bind(self.filepath())
            .execute(conn)
            .await?;
        Ok(())
    }

    pub async fn get_tile<T>(
        &self,
        conn: &mut T,
        z: u8,
        x: u32,
        y: u32,
    ) -> MbtResult<Option<Vec<u8>>>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let y = invert_y_value(z, y);
        let query = query! {"SELECT tile_data from tiles where zoom_level = ? AND tile_column = ? AND tile_row = ?", z, x, y};
        let row = query.fetch_optional(conn).await?;
        if let Some(row) = row {
            if let Some(tile_data) = row.tile_data {
                return Ok(Some(tile_data));
            }
        }
        Ok(None)
    }

    pub async fn insert_tiles(
        &self,
        conn: &mut SqliteConnection,
        mbt_type: MbtType,
        on_duplicate: CopyDuplicateMode,
        batch: &[(u8, u32, u32, Vec<u8>)],
    ) -> MbtResult<()> {
        debug!(
            "Inserting a batch of {} tiles into {mbt_type} / {on_duplicate}",
            batch.len()
        );
        let mut tx = conn.begin().await?;
        let (sql1, sql2) = Self::get_insert_sql(mbt_type, on_duplicate);
        if let Some(sql2) = sql2 {
            let sql2 = tx.prepare(&sql2).await?;
            for (_, _, _, tile_data) in batch {
                sql2.query().bind(tile_data).execute(&mut *tx).await?;
            }
        }
        let sql1 = tx.prepare(&sql1).await?;
        for (z, x, y, tile_data) in batch {
            let y = invert_y_value(*z, *y);
            sql1.query()
                .bind(z)
                .bind(x)
                .bind(y)
                .bind(tile_data)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    fn get_insert_sql(
        src_type: MbtType,
        on_duplicate: CopyDuplicateMode,
    ) -> (String, Option<String>) {
        let on_duplicate = on_duplicate.to_sql();
        match src_type {
            MbtType::Flat => (
                format!(
                    "
    INSERT {on_duplicate} INTO tiles (zoom_level, tile_column, tile_row, tile_data)
    VALUES (?1, ?2, ?3, ?4);"
                ),
                None,
            ),
            MbtType::FlatWithHash => (
                format!(
                    "
    INSERT {on_duplicate} INTO tiles_with_hash (zoom_level, tile_column, tile_row, tile_data, tile_hash)
    VALUES (?1, ?2, ?3, ?4, md5_hex(?4));"
                ),
                None,
            ),
            MbtType::Normalized { .. } => (
                format!(
                    "
    INSERT {on_duplicate} INTO map (zoom_level, tile_column, tile_row, tile_id)
    VALUES (?1, ?2, ?3, md5_hex(?4));"
                ),
                Some(format!(
                    "
    INSERT {on_duplicate} INTO images (tile_id, tile_data)
    VALUES (md5_hex(?1), ?1);"
                )),
            ),
        }
    }

    pub async fn open_and_detect_type(&self) -> MbtResult<MbtType> {
        let mut conn = self.open_readonly().await?;
        self.detect_type(&mut conn).await
    }
}

pub async fn attach_hash_fn(conn: &mut SqliteConnection) -> MbtResult<()> {
    let mut handle_lock = conn.lock_handle().await?;
    let handle = handle_lock.as_raw_handle().as_ptr();
    // Safety: we know that the handle is a SQLite connection is locked and is not used anywhere else.
    // The registered functions will be dropped when SQLX drops DB connection.
    let rc = unsafe { sqlite_hashes::rusqlite::Connection::from_handle(handle) }?;
    register_md5_function(&rc)?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub async fn open(filepath: &str) -> MbtResult<(SqliteConnection, Mbtiles)> {
        let mbt = Mbtiles::new(filepath)?;
        mbt.open().await.map(|conn| (conn, mbt))
    }
}

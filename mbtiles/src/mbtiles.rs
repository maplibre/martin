use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::path::Path;

#[cfg(feature = "cli")]
use clap::ValueEnum;
use enum_display::EnumDisplay;
use log::debug;
use sqlite_hashes::register_md5_function;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{query, Connection as _, SqliteConnection, SqliteExecutor};

use crate::errors::{MbtError, MbtResult};
use crate::{invert_y_value, MbtType};

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, EnumDisplay)]
#[enum_display(case = "Kebab")]
#[cfg_attr(feature = "cli", derive(ValueEnum))]
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

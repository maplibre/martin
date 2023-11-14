#![allow(clippy::missing_errors_doc)]

use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::path::Path;

#[cfg(feature = "cli")]
use clap::ValueEnum;
use enum_display::EnumDisplay;
use log::{debug, info, warn};
use martin_tile_utils::{Format, TileInfo};
use serde_json::Value;
use sqlite_hashes::register_md5_function;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{query, Connection as _, SqliteConnection, SqliteExecutor};
use tilejson::TileJSON;

use crate::errors::{MbtError, MbtResult};
use crate::MbtType;

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

    pub async fn detect_format<T>(&self, tilejson: &TileJSON, conn: &mut T) -> MbtResult<TileInfo>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let mut tile_info = None;
        let mut tested_zoom = -1_i64;

        // First, pick any random tile
        let query = query!("SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles WHERE zoom_level >= 0 LIMIT 1");
        let row = query.fetch_optional(&mut *conn).await?;
        if let Some(r) = row {
            tile_info = self.parse_tile(r.zoom_level, r.tile_column, r.tile_row, r.tile_data);
            tested_zoom = r.zoom_level.unwrap_or(-1);
        }

        // Afterwards, iterate over tiles in all allowed zooms and check for consistency
        for z in tilejson.minzoom.unwrap_or(0)..=tilejson.maxzoom.unwrap_or(18) {
            if i64::from(z) == tested_zoom {
                continue;
            }
            let query = query! {"SELECT tile_column, tile_row, tile_data FROM tiles WHERE zoom_level = ? LIMIT 1", z};
            let row = query.fetch_optional(&mut *conn).await?;
            if let Some(r) = row {
                match (
                    tile_info,
                    self.parse_tile(Some(z.into()), r.tile_column, r.tile_row, r.tile_data),
                ) {
                    (_, None) => {}
                    (None, new) => tile_info = new,
                    (Some(old), Some(new)) if old == new => {}
                    (Some(old), Some(new)) => {
                        return Err(MbtError::InconsistentMetadata(old, new));
                    }
                }
            }
        }

        if let Some(Value::String(fmt)) = tilejson.other.get("format") {
            let file = &self.filename;
            match (tile_info, Format::parse(fmt)) {
                (_, None) => {
                    warn!("Unknown format value in metadata: {fmt}");
                }
                (None, Some(fmt)) => {
                    if fmt.is_detectable() {
                        warn!("Metadata table sets detectable '{fmt}' tile format, but it could not be verified for file {file}");
                    } else {
                        info!("Using '{fmt}' tile format from metadata table in file {file}");
                    }
                    tile_info = Some(fmt.into());
                }
                (Some(info), Some(fmt)) if info.format == fmt => {
                    debug!("Detected tile format {info} matches metadata.format '{fmt}' in file {file}");
                }
                (Some(info), _) => {
                    warn!("Found inconsistency: metadata.format='{fmt}', but tiles were detected as {info:?} in file {file}. Tiles will be returned as {info:?}.");
                }
            }
        }

        if let Some(info) = tile_info {
            if info.format != Format::Mvt && tilejson.vector_layers.is_some() {
                warn!(
                    "{} has vector_layers metadata but non-vector tiles",
                    self.filename
                );
            }
            Ok(info)
        } else {
            Err(MbtError::NoTilesFound)
        }
    }

    fn parse_tile(
        &self,
        z: Option<i64>,
        x: Option<i64>,
        y: Option<i64>,
        tile: Option<Vec<u8>>,
    ) -> Option<TileInfo> {
        if let (Some(z), Some(x), Some(y), Some(tile)) = (z, x, y, tile) {
            let info = TileInfo::detect(&tile);
            if let Some(info) = info {
                debug!(
                    "Tile {z}/{x}/{} is detected as {info} in file {}",
                    (1 << z) - 1 - y,
                    self.filename,
                );
            }
            info
        } else {
            None
        }
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
        // let mut conn = self.pool.acquire().await?;
        let y = (1 << z) - 1 - y;
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

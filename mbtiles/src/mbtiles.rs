use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::path::Path;
use std::pin::Pin;

use enum_display::EnumDisplay;
use futures::Stream;
use log::debug;
use martin_tile_utils::{Tile, TileCoord};
use serde::{Deserialize, Serialize};
use sqlite_compressions::{register_bsdiffraw_functions, register_gzip_functions};
use sqlite_hashes::register_md5_functions;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection as _, Executor, Row, SqliteConnection, SqliteExecutor, Statement, query};

use crate::bindiff::PatchType;
use crate::errors::{MbtError, MbtResult};
use crate::{CopyDuplicateMode, MbtType, invert_y_value};

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize, EnumDisplay)]
#[enum_display(case = "Kebab")]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum MbtTypeCli {
    Flat,
    FlatWithHash,
    Normalized,
}

#[derive(Default, Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize, EnumDisplay)]
#[enum_display(case = "Kebab")]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum CopyType {
    #[default]
    All,
    Metadata,
    Tiles,
}

impl CopyType {
    #[must_use]
    pub fn copy_tiles(self) -> bool {
        matches!(self, Self::All | Self::Tiles)
    }
    #[must_use]
    pub fn copy_metadata(self) -> bool {
        matches!(self, Self::All | Self::Metadata)
    }
}

pub struct PatchFileInfo {
    pub mbt_type: MbtType,
    pub agg_tiles_hash: Option<String>,
    pub agg_tiles_hash_before_apply: Option<String>,
    pub agg_tiles_hash_after_apply: Option<String>,
    pub patch_type: Option<PatchType>,
}

/// A reference to an `MBTiles` file providing low-level database operations.
///
/// `Mbtiles` represents a reference to an [MBTiles](https://maplibre.org/martin/mbtiles-schema.html)
/// file without holding an open connection.
/// It provides methods for opening connections and performing tile operations directly.
///
/// # `MBTiles` Schema Types
///
/// `MBTiles` files can use one of three schema types (see [`MbtType`]):
/// - [`MbtType::Flat`] - Single table with all tiles, no deduplication
/// - [`MbtType::FlatWithHash`] - Single table with tiles and MD5 hashes
/// - [`MbtType::Normalized`] - Separate tables for deduplication via hashing
///
/// Use [`detect_type`](Self::detect_type) to determine which schema a file uses.
///
/// # Connection Management
///
/// `Mbtiles` requires you to manage `SQLite` connections explicitly. For concurrent
/// tile serving, consider using [`crate::MbtilesPool`] instead, which provides connection pooling.
///
/// # Examples
///
/// ## Reading tiles from an existing file
///
/// > [!NOTE]
/// > Note that there are both [osgeos' Tile Map Service](https://wiki.openstreetmap.org/wiki/TMS) and [xyz Slippy map tilenames](https://wiki.openstreetmap.org/wiki/Slippy_map_tilenames) tiling shemes.
/// > They differ only in if the y coordinate direction.
/// > **The default in mapbox and maplibre is xyz.***
/// > **The default in mbtiles generation like plantitler is tms.***
/// >
/// > You can use [`mbtiles::invert_y_value`] to convert them.
///
/// ```
/// use mbtiles::Mbtiles;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let mbt = Mbtiles::new("world.mbtiles")?;
/// let mut conn = mbt.open_readonly().await?;
///
/// // Get a tile at zoom 4, x=5, y=6
/// if let Some(tile_data) = mbt.get_tile(&mut conn, 4, 5, 6).await? {
///     println!("Retrieved tile: {} bytes", tile_data.len());
/// }
/// # Ok(())
/// # }
/// ```
///
/// ## Creating and writing tiles to a new file
///
/// ```
/// use mbtiles::{Mbtiles, MbtType, CopyDuplicateMode};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let mbt = Mbtiles::new("output.mbtiles")?;
/// let mut conn = mbt.open_or_new().await?;
///
/// // Initialize with flat schema
/// mbtiles::init_mbtiles_schema(&mut conn, MbtType::Flat).await?;
///
/// // Insert a batch of tiles
/// let tiles = vec![
///     (0, 0, 0, vec![1, 2, 3, 4]),  // zoom, x, y, data
///     (1, 0, 0, vec![5, 6, 7, 8]),
/// ];
/// mbt.insert_tiles(&mut conn, MbtType::Flat, CopyDuplicateMode::Override, &tiles).await?;
/// # Ok(())
/// # }
/// ```
///
/// ## Streaming all tiles
///
/// ```
/// use mbtiles::Mbtiles;
/// use futures::StreamExt;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let mbt = Mbtiles::new("world.mbtiles")?;
/// let mut conn = mbt.open_readonly().await?;
///
/// let mut stream = mbt.stream_tiles(&mut conn);
/// while let Some(tile) = stream.next().await {
///     let (coord, data) = tile?;
///     println!("Tile at {}/{}/{}: {} bytes", coord.z, coord.x, coord.y, data.map(|bytes| bytes.len()).unwrap_or_default());
/// }
/// # Ok(())
/// # }
/// ```
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
    /// Creates a reference to an mbtiles file.
    ///
    /// This does not open the file, nor check if it exists.
    /// For this, please use the [`Mbtiles::open`],  [`Mbtiles::open_or_new`] or [`Mbtiles::open_readonly`] method respectively.
    ///
    /// # Errors
    /// Returns an error if the filepath contains unsupported characters.
    ///
    /// # Examples
    /// ```
    /// use mbtiles::Mbtiles;
    ///
    /// let mbtiles = Mbtiles::new("example.mbtiles").unwrap();
    /// ```
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

    /// Opens an existing `MBTiles` file in read-write mode.
    ///
    /// Opens a connection to the file for both reading and writing operations.
    /// The file must already exist; use [`open_or_new`](Self::open_or_new) to create
    /// a new file if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file does not exist
    /// - The file cannot be opened (permissions, corruption, etc.)
    /// - The file is not a valid `SQLite` database
    ///
    /// # Examples
    /// ```
    /// use mbtiles::Mbtiles;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mbtiles = Mbtiles::new("existing.mbtiles")?;
    /// let mut conn = mbtiles.open().await?;
    ///
    /// // Can now read and write tiles
    /// # Ok(())
    /// # }
    /// ```
    pub async fn open(&self) -> MbtResult<SqliteConnection> {
        debug!("Opening w/ defaults {self}");
        let opt = SqliteConnectOptions::new().filename(self.filepath());
        Self::open_int(&opt).await
    }

    /// Opens an `MBTiles` file in read-write mode, creating it if it doesn't exist.
    ///
    /// If the file exists, opens it for reading and writing. If it doesn't exist,
    /// creates a new empty `SQLite` database file. After creation, you must initialize
    /// the schema using [`init_mbtiles_schema`](crate::init_mbtiles_schema).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be created or opened (permissions, disk space, etc.)
    /// - An existing file is not a valid `SQLite` database
    ///
    /// # Examples
    /// ```
    /// use mbtiles::{Mbtiles, MbtType};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mbtiles = Mbtiles::new("new.mbtiles")?;
    /// let mut conn = mbtiles.open_or_new().await?;
    ///
    /// // Initialize schema for a new file
    /// mbtiles::init_mbtiles_schema(&mut conn, MbtType::Flat).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn open_or_new(&self) -> MbtResult<SqliteConnection> {
        debug!("Opening or creating {self}");
        let opt = SqliteConnectOptions::new()
            .filename(self.filepath())
            .create_if_missing(true);
        Self::open_int(&opt).await
    }

    /// Opens an existing `MBTiles` file in read-only mode.
    ///
    /// Opens a connection that can only read data. This is useful for:
    /// - Serving tiles in production (prevents accidental modifications)
    /// - Reading from write-protected files
    /// - Allowing multiple processes to read simultaneously
    ///
    /// For concurrent access from a single process, consider using [`crate::MbtilesPool`] instead.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file does not exist
    /// - The file cannot be opened (permissions, corruption, etc.)
    /// - The file is not a valid `SQLite` database
    ///
    /// # Examples
    /// ```
    /// use mbtiles::Mbtiles;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mbtiles = Mbtiles::new("world.mbtiles")?;
    /// let mut conn = mbtiles.open_readonly().await?;
    ///
    /// // Can read tiles but cannot write
    /// let tile = mbtiles.get_tile(&mut conn, 0, 0, 0).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn open_readonly(&self) -> MbtResult<SqliteConnection> {
        debug!("Opening as readonly {self}");
        let opt = SqliteConnectOptions::new()
            .filename(self.filepath())
            .read_only(true);
        Self::open_int(&opt).await
    }

    async fn open_int(opt: &SqliteConnectOptions) -> Result<SqliteConnection, MbtError> {
        let mut conn = SqliteConnection::connect_with(opt).await?;
        attach_sqlite_fn(&mut conn).await?;
        Ok(conn)
    }

    /// The filepath of the mbtiles database
    #[must_use]
    pub fn filepath(&self) -> &str {
        &self.filepath
    }

    /// The filename of the mbtiles database
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

    /// Stream over coordinates of all tiles in the database.
    ///
    /// No particular order is guaranteed.
    ///
    /// <div class="warning">
    ///
    /// **Note:** The returned [`Stream`] holds a mutable reference to the given
    /// connection, making it unusable for anything else until the stream
    /// is dropped.
    ///
    /// </div>
    pub fn stream_coords<'e, T>(
        &self,
        conn: &'e mut T,
    ) -> Pin<Box<dyn Stream<Item = MbtResult<TileCoord>> + Send + 'e>>
    where
        &'e mut T: SqliteExecutor<'e>,
    {
        use futures::StreamExt;

        let query = query! {"SELECT zoom_level, tile_column, tile_row FROM tiles"};
        let stream = query.fetch(conn);

        // We only need `&self` for `self.filepath`, which in turn we only
        // need to create proper `MbtError::InvalidTileIndex`es.
        // Cloning the filepath allows us to drop [Mbtiles] instance while returned
        // stream is still alive.
        let filepath = self.filepath.clone();

        Box::pin(stream.map(move |result| {
            result.map_err(MbtError::from).and_then(|row| {
                let z = row.zoom_level;
                let x = row.tile_column;
                let y = row.tile_row;
                let coord = parse_tile_index(z, x, y).ok_or_else(|| {
                    MbtError::InvalidTileIndex(
                        filepath.clone(),
                        format!("{z:?}"),
                        format!("{x:?}"),
                        format!("{y:?}"),
                    )
                })?;
                Ok(coord)
            })
        }))
    }

    /// Returns a stream over all tiles in the database.
    ///
    /// No particular order is guaranteed.
    ///
    /// <div class="warning">
    ///
    /// **Note:** The returned [`Stream`] holds a mutable reference to the given
    /// connection, making it unusable for anything else until the stream
    /// is dropped.
    ///
    /// </div>
    pub fn stream_tiles<'e, T>(
        &self,
        conn: &'e mut T,
    ) -> Pin<Box<dyn Stream<Item = MbtResult<Tile>> + Send + 'e>>
    where
        &'e mut T: SqliteExecutor<'e>,
    {
        use futures::StreamExt;

        let query = query! {"SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles"};
        let stream = query.fetch(conn);
        let filepath = self.filepath.clone();

        Box::pin(stream.map(move |result| {
            result.map_err(MbtError::from).and_then(|row| {
                let z = row.zoom_level;
                let x = row.tile_column;
                let y = row.tile_row;
                let coord = parse_tile_index(z, x, y).ok_or_else(|| {
                    MbtError::InvalidTileIndex(
                        filepath.clone(),
                        format!("{z:?}"),
                        format!("{x:?}"),
                        format!("{y:?}"),
                    )
                })?;
                Ok((coord, row.tile_data))
            })
        }))
    }

    /// Retrieves a single tile from the database by its coordinates.
    ///
    /// Returns the raw tile data as a byte vector if the tile exists at the given
    /// zoom level and x/y coordinates. Returns `None` if no tile exists at those
    /// coordinates.
    ///
    /// # Coordinate System
    ///
    /// Coordinates use the XYZ tile scheme where:
    /// - `z` is the zoom level (0-30)
    /// - `x` is the column (0 to 2^z - 1)
    /// - `y` is the row in XYZ format (0 at top, increases southward)
    ///
    /// > [!NOTE]
    /// > MBTiles files internally use [osgeos' Tile Map Service](https://wiki.openstreetmap.org/wiki/TMS) coordinates (0 at bottom).
    /// > This method handles the conversion automatically as maplibre/mapbox expect this.
    ///
    /// # Performance
    ///
    /// If you also need the tile hash, use [`get_tile_and_hash`](Self::get_tile_and_hash)
    /// to fetch both in a single query.
    ///
    /// # Coordinate System
    ///
    /// Coordinates use the [xyz Slippy map tilenames](https://wiki.openstreetmap.org/wiki/Slippy_map_tilenames) tile scheme where:
    /// - `z` is the zoom level (0-30)
    /// - `x` is the column (0 to 2^z - 1)
    /// - `y` is the row in XYZ format (0 at top, increases southward)
    ///
    /// > [!NOTE]
    /// > MBTiles files internally use [osgeos' Tile Map Service](https://wiki.openstreetmap.org/wiki/TMS) coordinates (0 at bottom).
    /// > This method handles the conversion automatically as maplibre/mapbox expect this.
    ///
    /// # Examples
    ///
    /// ```
    /// use mbtiles::Mbtiles;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mbt = Mbtiles::new("world.mbtiles")?;
    /// let mut conn = mbt.open_readonly().await?;
    ///
    /// // Get tile at zoom 4, x=5, y=6
    /// match mbt.get_tile(&mut conn, 4, 5, 6).await? {
    ///     Some(data) => println!("Tile size: {} bytes", data.len()),
    ///     None => println!("Tile not found"),
    /// }
    /// # Ok(())
    /// # }
    /// ```
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
        if let Some(row) = row
            && let Some(tile_data) = row.tile_data
        {
            return Ok(Some(tile_data));
        }
        Ok(None)
    }

    /// Retrieves a tile and its hash from the database.
    ///
    /// Returns both the tile data and its hash value (if available) for the tile
    /// at the given coordinates. The hash behavior depends on the schema type:
    ///
    /// - [`MbtType::Flat`]: Hash is always `None` (no hash column exists)
    /// - [`MbtType::FlatWithHash`]: Returns the stored MD5 hash
    /// - [`MbtType::Normalized`]: Returns the `tile_id` (MD5 hash) from the images table
    ///
    /// # Returns
    ///
    /// - `Ok(Some((data, hash)))` if the tile exists
    /// - `Ok(None)` if no tile exists at the coordinates
    /// - `Err(_)` on database errors or schema mismatches
    ///
    /// # Performance
    ///
    /// If you don't need the hash, use [`get_tile`](Self::get_tile) instead to avoid
    /// the overhead of hash retrieval.
    ///
    /// # Coordinate System
    ///
    /// Coordinates use the [xyz Slippy map tilenames](https://wiki.openstreetmap.org/wiki/Slippy_map_tilenames) tile scheme where:
    /// - `z` is the zoom level (0-30)
    /// - `x` is the column (0 to 2^z - 1)
    /// - `y` is the row in XYZ format (0 at top, increases southward)
    ///
    /// > [!NOTE]
    /// > MBTiles files internally use [osgeos' Tile Map Service](https://wiki.openstreetmap.org/wiki/TMS) coordinates (0 at bottom).
    /// > This method handles the conversion automatically as maplibre/mapbox expect this.
    ///
    /// # Examples
    ///
    /// ```
    /// use mbtiles::{Mbtiles, MbtType};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mbt = Mbtiles::new("tiles.mbtiles")?;
    /// let mut conn = mbt.open_readonly().await?;
    /// let mbt_type = mbt.detect_type(&mut conn).await?;
    ///
    /// match mbt.get_tile_and_hash(&mut conn, mbt_type, 4, 5, 6).await? {
    ///     Some((data, Some(hash))) => {
    ///         println!("Tile: {} bytes, hash: {}", data.len(), hash);
    ///     }
    ///     Some((data, None)) => {
    ///         println!("Tile: {} bytes (no hash available)", data.len());
    ///     }
    ///     None => println!("Tile not found"),
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_tile_and_hash(
        &self,
        conn: &mut SqliteConnection,
        mbt_type: MbtType,
        z: u8,
        x: u32,
        y: u32,
    ) -> MbtResult<Option<(Vec<u8>, Option<String>)>> {
        let sql = Self::get_tile_and_hash_sql(mbt_type);
        let y = invert_y_value(z, y);
        let Some(row) = query(sql)
            .bind(z)
            .bind(x)
            .bind(y)
            .fetch_optional(conn)
            .await?
        else {
            return Ok(None);
        };
        Ok(Some((row.get(0), row.get(1))))
    }

    /// sql query for getting tile and hash
    ///
    /// For [`MbtType::Flat`] accessing the hash is not possible, so the SQL query explicitly returns `NULL as tile_hash`.
    fn get_tile_and_hash_sql(mbt_type: MbtType) -> &'static str {
        match mbt_type {
            MbtType::Flat => {
                "SELECT tile_data, NULL as tile_hash from tiles where zoom_level = ? AND tile_column = ? AND tile_row = ?"
            }
            MbtType::FlatWithHash | MbtType::Normalized { hash_view: true } => {
                "SELECT tile_data, tile_hash from tiles_with_hash where zoom_level = ? AND tile_column = ? AND tile_row = ?"
            }
            MbtType::Normalized { hash_view: false } => {
                "SELECT images.tile_data, images.tile_id AS tile_hash FROM map JOIN images ON map.tile_id = images.tile_id  where map.zoom_level = ? AND map.tile_column = ? AND map.tile_row = ?"
            }
        }
    }

    /// Inserts the batch of tiles into the mbtiles database.
    ///
    /// # Example
    ///
    /// ```
    /// use mbtiles::MbtType;
    /// use mbtiles::CopyDuplicateMode;
    /// use mbtiles::Mbtiles;
    ///
    /// # async fn insert_tiles_example() {
    /// let mbtiles = Mbtiles::new("example.mbtiles").unwrap();
    /// let mut conn = mbtiles.open().await.unwrap();
    ///
    /// let mbt_type = mbtiles.detect_type(&mut conn).await.unwrap();
    /// let batch = vec![
    ///     (0, 0, 0, vec![0, 1, 2, 3]),
    ///     (0, 1, 0, vec![4, 5, 6, 7]),
    /// ];
    /// mbtiles.insert_tiles(&mut conn, mbt_type, CopyDuplicateMode::Ignore, &batch).await.unwrap();
    /// # }
    /// ```
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

    /// Check if a tile exists in the database.
    ///
    /// This method is slightly faster than [`Mbtiles::get_tile_and_hash`] and [`Mbtiles::get_tile`]
    /// because it only checks if the tile exists but does not retrieve tile data.
    /// Most of the time you would want to use the other two functions.
    pub async fn contains(
        &self,
        conn: &mut SqliteConnection,
        mbt_type: MbtType,
        z: u8,
        x: u32,
        y: u32,
    ) -> MbtResult<bool> {
        let table = match mbt_type {
            MbtType::Flat => "tiles",
            MbtType::FlatWithHash => "tiles_with_hash",
            MbtType::Normalized { .. } => "map",
        };
        let sql = format!(
            "SELECT 1 from {table} where zoom_level = ? AND tile_column = ? AND tile_row = ?"
        );
        let row = query(&sql)
            .bind(z)
            .bind(x)
            .bind(invert_y_value(z, y))
            .fetch_optional(conn)
            .await?;
        Ok(row.is_some())
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
}

pub async fn attach_sqlite_fn(conn: &mut SqliteConnection) -> MbtResult<()> {
    let mut handle_lock = conn.lock_handle().await?;
    let handle = handle_lock.as_raw_handle().as_ptr();
    // Safety: we know that the handle is a SQLite connection is locked and is not used anywhere else.
    // The registered functions will be dropped when SQLX drops DB connection.
    let rc = unsafe { sqlite_hashes::rusqlite::Connection::from_handle(handle) }?;
    register_md5_functions(&rc)?;
    register_bsdiffraw_functions(&rc)?;
    register_gzip_functions(&rc)?;
    Ok(())
}

fn parse_tile_index(z: Option<i64>, x: Option<i64>, y: Option<i64>) -> Option<TileCoord> {
    let z: u8 = z?.try_into().ok()?;
    let x: u32 = x?.try_into().ok()?;
    let y: u32 = y?.try_into().ok()?;

    // Inverting `y` value can panic if it is greater than `(1 << z) - 1`,
    // so we must ensure that it is vald first.
    TileCoord::is_possible_on_zoom_level(z, x, y)
        .then(|| TileCoord::new_unchecked(z, x, invert_y_value(z, y)))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub async fn open(filepath: &str) -> MbtResult<(SqliteConnection, Mbtiles)> {
        let mbt = Mbtiles::new(filepath)?;
        mbt.open().await.map(|conn| (conn, mbt))
    }
}

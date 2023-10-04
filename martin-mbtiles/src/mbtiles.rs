#![allow(clippy::missing_errors_doc)]

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fmt::Display;
use std::path::Path;
use std::str::FromStr;

#[cfg(feature = "cli")]
use clap::ValueEnum;
use enum_display::EnumDisplay;
use futures::TryStreamExt;
use log::{debug, info, warn};
use martin_tile_utils::{Format, TileInfo};
use serde::ser::SerializeStruct;
use serde::Serialize;
use serde_json::{Value as JSONValue, Value};
use sqlite_hashes::register_md5_function;
use sqlx::sqlite::{SqliteConnectOptions, SqliteRow};
use sqlx::{query, Connection as _, Row, SqliteConnection, SqliteExecutor};
use tilejson::{tilejson, Bounds, Center, TileJSON};

use crate::errors::{MbtError, MbtResult};
use crate::queries::{
    is_flat_tables_type, is_flat_with_hash_tables_type, is_normalized_tables_type,
};
use crate::MbtError::{
    AggHashMismatch, AggHashValueNotFound, FailedIntegrityCheck, IncorrectTileHash,
};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Metadata {
    pub id: String,
    #[serde(serialize_with = "serialize_ti")]
    pub tile_info: TileInfo,
    pub layer_type: Option<String>,
    pub tilejson: TileJSON,
    pub json: Option<JSONValue>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn serialize_ti<S>(ti: &TileInfo, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let mut s = serializer.serialize_struct("TileInfo", 2)?;
    s.serialize_field("format", &ti.format.to_string())?;
    s.serialize_field(
        "encoding",
        match ti.encoding.content_encoding() {
            None => "",
            Some(v) => v,
        },
    )?;
    s.end()
}

/// Metadata key for the aggregate tiles hash value
pub const AGG_TILES_HASH: &str = "agg_tiles_hash";

/// Metadata key for a diff file,
/// describing the eventual AGG_TILES_HASH value once the diff is applied
pub const AGG_TILES_HASH_IN_DIFF: &str = "agg_tiles_hash_after_apply";

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumDisplay)]
#[enum_display(case = "Kebab")]
#[cfg_attr(feature = "cli", derive(ValueEnum))]
pub enum MbtType {
    Flat,
    FlatWithHash,
    Normalized,
}

#[derive(PartialEq, Eq, Default, Debug, Clone, EnumDisplay)]
#[enum_display(case = "Kebab")]
#[cfg_attr(feature = "cli", derive(ValueEnum))]
pub enum IntegrityCheckType {
    #[default]
    Quick,
    Full,
    Off,
}

#[derive(Clone, Debug)]
pub struct Mbtiles {
    filepath: String,
    filename: String,
}

impl Display for Mbtiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

    fn to_val<V, E: Display>(&self, val: Result<V, E>, title: &str) -> Option<V> {
        match val {
            Ok(v) => Some(v),
            Err(err) => {
                let name = &self.filename;
                warn!("Unable to parse metadata {title} value in {name}: {err}");
                None
            }
        }
    }

    /// Attach this MBTiles file to the given SQLite connection as a given name
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

    /// Get a single metadata value from the metadata table
    pub async fn get_metadata_value<T>(&self, conn: &mut T, key: &str) -> MbtResult<Option<String>>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let query = query!("SELECT value from metadata where name = ?", key);
        let row = query.fetch_optional(conn).await?;
        if let Some(row) = row {
            if let Some(value) = row.value {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    /// Get the aggregate tiles hash value from the metadata table
    pub async fn get_agg_tiles_hash<T>(&self, conn: &mut T) -> MbtResult<Option<String>>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        self.get_metadata_value(&mut *conn, AGG_TILES_HASH).await
    }

    pub async fn set_metadata_value<T>(
        &self,
        conn: &mut T,
        key: &str,
        value: Option<String>,
    ) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        if let Some(value) = value {
            query!(
                "INSERT OR REPLACE INTO metadata(name, value) VALUES(?, ?)",
                key,
                value
            )
            .execute(conn)
            .await?;
        } else {
            query!("DELETE FROM metadata WHERE name=?", key)
                .execute(conn)
                .await?;
        }
        Ok(())
    }

    pub async fn get_metadata<T>(&self, conn: &mut T) -> MbtResult<Metadata>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let (tj, layer_type, json) = self.parse_metadata(conn).await?;

        Ok(Metadata {
            id: self.filename.to_string(),
            tile_info: self.detect_format(&tj, conn).await?,
            tilejson: tj,
            layer_type,
            json,
        })
    }

    async fn parse_metadata<T>(
        &self,
        conn: &mut T,
    ) -> MbtResult<(TileJSON, Option<String>, Option<Value>)>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let query = query!("SELECT name, value FROM metadata WHERE value IS NOT ''");
        let mut rows = query.fetch(conn);

        let mut tj = tilejson! { tiles: vec![] };
        let mut layer_type: Option<String> = None;
        let mut json: Option<JSONValue> = None;

        while let Some(row) = rows.try_next().await? {
            if let (Some(name), Some(value)) = (row.name, row.value) {
                match name.as_ref() {
                    "name" => tj.name = Some(value),
                    "version" => tj.version = Some(value),
                    "bounds" => tj.bounds = self.to_val(Bounds::from_str(value.as_str()), &name),
                    "center" => tj.center = self.to_val(Center::from_str(value.as_str()), &name),
                    "minzoom" => tj.minzoom = self.to_val(value.parse(), &name),
                    "maxzoom" => tj.maxzoom = self.to_val(value.parse(), &name),
                    "description" => tj.description = Some(value),
                    "attribution" => tj.attribution = Some(value),
                    "type" => layer_type = Some(value),
                    "legend" => tj.legend = Some(value),
                    "template" => tj.template = Some(value),
                    "json" => json = self.to_val(serde_json::from_str(&value), &name),
                    "format" | "generator" => {
                        tj.other.insert(name, Value::String(value));
                    }
                    _ => {
                        let file = &self.filename;
                        info!("{file} has an unrecognized metadata value {name}={value}");
                        tj.other.insert(name, Value::String(value));
                    }
                }
            }
        }

        if let Some(JSONValue::Object(obj)) = &mut json {
            if let Some(value) = obj.remove("vector_layers") {
                if let Ok(v) = serde_json::from_value(value) {
                    tj.vector_layers = Some(v);
                } else {
                    warn!(
                        "Unable to parse metadata vector_layers value in {}",
                        self.filename
                    );
                }
            }
        }

        Ok((tj, layer_type, json))
    }

    async fn detect_format<T>(&self, tilejson: &TileJSON, conn: &mut T) -> MbtResult<TileInfo>
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

    pub async fn detect_type<T>(&self, conn: &mut T) -> MbtResult<MbtType>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        debug!("Detecting MBTiles type for {self}");
        let mbt_type = if is_normalized_tables_type(&mut *conn).await? {
            MbtType::Normalized
        } else if is_flat_with_hash_tables_type(&mut *conn).await? {
            MbtType::FlatWithHash
        } else if is_flat_tables_type(&mut *conn).await? {
            MbtType::Flat
        } else {
            return Err(MbtError::InvalidDataFormat(self.filepath.clone()));
        };

        self.check_for_uniqueness_constraint(&mut *conn, mbt_type)
            .await?;

        Ok(mbt_type)
    }

    async fn check_for_uniqueness_constraint<T>(
        &self,
        conn: &mut T,
        mbt_type: MbtType,
    ) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let table_name = match mbt_type {
            MbtType::Flat => "tiles",
            MbtType::FlatWithHash => "tiles_with_hash",
            MbtType::Normalized => "map",
        };

        let indexes = query("SELECT name FROM pragma_index_list(?) WHERE [unique] = 1")
            .bind(table_name)
            .fetch_all(&mut *conn)
            .await?;

        // Ensure there is some index on tiles that has a unique constraint on (zoom_level, tile_row, tile_column)
        for index in indexes {
            let mut unique_idx_cols = HashSet::new();
            let rows = query("SELECT DISTINCT name FROM pragma_index_info(?)")
                .bind(index.get::<String, _>("name"))
                .fetch_all(&mut *conn)
                .await?;

            for row in rows {
                unique_idx_cols.insert(row.get("name"));
            }

            if unique_idx_cols
                .symmetric_difference(&HashSet::from([
                    "zoom_level".to_string(),
                    "tile_column".to_string(),
                    "tile_row".to_string(),
                ]))
                .collect::<Vec<_>>()
                .is_empty()
            {
                return Ok(());
            }
        }

        Err(MbtError::NoUniquenessConstraint(self.filepath.clone()))
    }

    /// Perform `SQLite` internal integrity check
    pub async fn check_integrity<T>(
        &self,
        conn: &mut T,
        integrity_check: IntegrityCheckType,
    ) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        if integrity_check == IntegrityCheckType::Off {
            info!("Skipping integrity check for {self}");
            return Ok(());
        }

        let sql = if integrity_check == IntegrityCheckType::Full {
            "PRAGMA integrity_check;"
        } else {
            "PRAGMA quick_check;"
        };

        let result: Vec<String> = query(sql)
            .map(|row: SqliteRow| row.get(0))
            .fetch_all(&mut *conn)
            .await?;

        if result.len() > 1
            || result.get(0).ok_or(FailedIntegrityCheck(
                self.filepath.to_string(),
                vec!["SQLite could not perform integrity check".to_string()],
            ))? != "ok"
        {
            return Err(FailedIntegrityCheck(self.filepath().to_string(), result));
        }

        info!("{integrity_check:?} integrity check passed for {self}");
        Ok(())
    }

    pub async fn check_agg_tiles_hashes<T>(&self, conn: &mut T) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let Some(stored) = self.get_agg_tiles_hash(&mut *conn).await? else {
            return Err(AggHashValueNotFound(self.filepath().to_string()));
        };
        let computed = calc_agg_tiles_hash(&mut *conn).await?;
        if stored != computed {
            let file = self.filepath().to_string();
            return Err(AggHashMismatch(computed, stored, file));
        }

        info!("The agg_tiles_hashes={computed} has been verified for {self}");
        Ok(())
    }

    /// Compute new aggregate tiles hash and save it to the metadata table (if needed)
    pub async fn update_agg_tiles_hash<T>(&self, conn: &mut T) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let old_hash = self.get_agg_tiles_hash(&mut *conn).await?;
        let hash = calc_agg_tiles_hash(&mut *conn).await?;
        if old_hash.as_ref() == Some(&hash) {
            info!("agg_tiles_hash is already set to the correct value `{hash}` in {self}");
            Ok(())
        } else {
            if let Some(old_hash) = old_hash {
                info!("Updating agg_tiles_hash from {old_hash} to {hash} in {self}");
            } else {
                info!("Creating new metadata value agg_tiles_hash = {hash} in {self}");
            }
            self.set_metadata_value(&mut *conn, AGG_TILES_HASH, Some(hash))
                .await
        }
    }

    pub async fn check_each_tile_hash<T>(&self, conn: &mut T) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        // Note that hex() always returns upper-case HEX values
        let sql = match self.detect_type(&mut *conn).await? {
            MbtType::Flat => {
                info!("Skipping per-tile hash validation because this is a flat MBTiles file");
                return Ok(());
            }
            MbtType::FlatWithHash => {
                "SELECT expected, computed FROM (
                    SELECT
                        upper(tile_hash) AS expected,
                        hex(md5(tile_data)) AS computed
                    FROM tiles_with_hash
                ) AS t
                WHERE expected != computed
                LIMIT 1;"
            }
            MbtType::Normalized => {
                "SELECT expected, computed FROM (
                    SELECT
                        upper(tile_id) AS expected,
                        hex(md5(tile_data)) AS computed
                    FROM images
                ) AS t
                WHERE expected != computed
                LIMIT 1;"
            }
        };

        query(sql)
            .fetch_optional(&mut *conn)
            .await?
            .map_or(Ok(()), |v| {
                Err(IncorrectTileHash(
                    self.filepath().to_string(),
                    v.get(0),
                    v.get(1),
                ))
            })?;

        info!("All tile hashes are valid for {self}");
        Ok(())
    }
}

/// Compute the hash of the combined tiles in the mbtiles file tiles table/view.
/// This should work on all mbtiles files perf `MBTiles` specification.
async fn calc_agg_tiles_hash<T>(conn: &mut T) -> MbtResult<String>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!("Calculating agg_tiles_hash");
    let query = query(
        // The md5_concat func will return NULL if there are no rows in the tiles table.
        // For our use case, we will treat it as an empty string, and hash that.
        // `tile_data` values must be stored as a blob per MBTiles spec
        // `md5` functions will fail if the value is not text/blob/null
        "SELECT
         hex(
           coalesce(
             md5_concat(
               cast(zoom_level AS text),
               cast(tile_column AS text),
               cast(tile_row AS text),
               tile_data
             ),
             md5('')
           )
         )
         FROM tiles
         ORDER BY zoom_level, tile_column, tile_row;",
    );
    Ok(query.fetch_one(conn).await?.get::<String, _>(0))
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
mod tests {
    use std::collections::HashMap;

    use martin_tile_utils::Encoding;
    use sqlx::Executor as _;
    use tilejson::VectorLayer;

    use super::*;

    async fn open(filepath: &str) -> MbtResult<(SqliteConnection, Mbtiles)> {
        let mbt = Mbtiles::new(filepath)?;
        mbt.open().await.map(|conn| (conn, mbt))
    }

    #[actix_rt::test]
    async fn mbtiles_meta() -> MbtResult<()> {
        let filepath = "../tests/fixtures/mbtiles/geography-class-jpg.mbtiles";
        let mbt = Mbtiles::new(filepath)?;
        assert_eq!(mbt.filepath(), filepath);
        assert_eq!(mbt.filename(), "geography-class-jpg");
        Ok(())
    }

    #[actix_rt::test]
    async fn metadata_jpeg() -> MbtResult<()> {
        let (mut conn, mbt) = open("../tests/fixtures/mbtiles/geography-class-jpg.mbtiles").await?;
        let metadata = mbt.get_metadata(&mut conn).await?;
        let tj = metadata.tilejson;

        assert_eq!(tj.description.unwrap(), "One of the example maps that comes with TileMill - a bright & colorful world map that blends retro and high-tech with its folded paper texture and interactive flag tooltips. ");
        assert!(tj.legend.unwrap().starts_with("<div style="));
        assert_eq!(tj.maxzoom.unwrap(), 1);
        assert_eq!(tj.minzoom.unwrap(), 0);
        assert_eq!(tj.name.unwrap(), "Geography Class");
        assert_eq!(tj.template.unwrap(),"{{#__location__}}{{/__location__}}{{#__teaser__}}<div style=\"text-align:center;\">\n\n<img src=\"data:image/png;base64,{{flag_png}}\" style=\"-moz-box-shadow:0px 1px 3px #222;-webkit-box-shadow:0px 1px 5px #222;box-shadow:0px 1px 3px #222;\"><br>\n<strong>{{admin}}</strong>\n\n</div>{{/__teaser__}}{{#__full__}}{{/__full__}}");
        assert_eq!(tj.version.unwrap(), "1.0.0");
        assert_eq!(metadata.id, "geography-class-jpg");
        assert_eq!(metadata.tile_info, Format::Jpeg.into());
        Ok(())
    }

    #[actix_rt::test]
    async fn metadata_mvt() -> MbtResult<()> {
        let (mut conn, mbt) = open("../tests/fixtures/mbtiles/world_cities.mbtiles").await?;
        let metadata = mbt.get_metadata(&mut conn).await?;
        let tj = metadata.tilejson;

        assert_eq!(tj.maxzoom.unwrap(), 6);
        assert_eq!(tj.minzoom.unwrap(), 0);
        assert_eq!(tj.name.unwrap(), "Major cities from Natural Earth data");
        assert_eq!(tj.version.unwrap(), "2");
        assert_eq!(
            tj.vector_layers,
            Some(vec![VectorLayer {
                id: "cities".to_string(),
                fields: vec![("name".to_string(), "String".to_string())]
                    .into_iter()
                    .collect(),
                description: Some(String::new()),
                minzoom: Some(0),
                maxzoom: Some(6),
                other: HashMap::default()
            }])
        );
        assert_eq!(metadata.id, "world_cities");
        assert_eq!(
            metadata.tile_info,
            TileInfo::new(Format::Mvt, Encoding::Gzip)
        );
        assert_eq!(metadata.layer_type, Some("overlay".to_string()));
        Ok(())
    }

    #[actix_rt::test]
    async fn metadata_get_key() -> MbtResult<()> {
        let (mut conn, mbt) = open("../tests/fixtures/mbtiles/world_cities.mbtiles").await?;

        let res = mbt.get_metadata_value(&mut conn, "bounds").await?.unwrap();
        assert_eq!(res, "-123.123590,-37.818085,174.763027,59.352706");
        let res = mbt.get_metadata_value(&mut conn, "name").await?.unwrap();
        assert_eq!(res, "Major cities from Natural Earth data");
        let res = mbt.get_metadata_value(&mut conn, "maxzoom").await?.unwrap();
        assert_eq!(res, "6");
        let res = mbt.get_metadata_value(&mut conn, "nonexistent_key").await?;
        assert_eq!(res, None);
        let res = mbt.get_metadata_value(&mut conn, "").await?;
        assert_eq!(res, None);
        Ok(())
    }

    #[actix_rt::test]
    async fn metadata_set_key() -> MbtResult<()> {
        let (mut conn, mbt) = open("file:metadata_set_key_mem_db?mode=memory&cache=shared").await?;

        conn.execute("CREATE TABLE metadata (name text NOT NULL PRIMARY KEY, value text);")
            .await?;

        mbt.set_metadata_value(&mut conn, "bounds", Some("0.0, 0.0, 0.0, 0.0".to_string()))
            .await?;
        assert_eq!(
            mbt.get_metadata_value(&mut conn, "bounds").await?.unwrap(),
            "0.0, 0.0, 0.0, 0.0"
        );

        mbt.set_metadata_value(
            &mut conn,
            "bounds",
            Some("-123.123590,-37.818085,174.763027,59.352706".to_string()),
        )
        .await?;
        assert_eq!(
            mbt.get_metadata_value(&mut conn, "bounds").await?.unwrap(),
            "-123.123590,-37.818085,174.763027,59.352706"
        );

        mbt.set_metadata_value(&mut conn, "bounds", None).await?;
        assert_eq!(mbt.get_metadata_value(&mut conn, "bounds").await?, None);

        Ok(())
    }

    #[actix_rt::test]
    async fn detect_type() -> MbtResult<()> {
        let (mut conn, mbt) = open("../tests/fixtures/mbtiles/world_cities.mbtiles").await?;
        let res = mbt.detect_type(&mut conn).await?;
        assert_eq!(res, MbtType::Flat);

        let (mut conn, mbt) = open("../tests/fixtures/mbtiles/zoomed_world_cities.mbtiles").await?;
        let res = mbt.detect_type(&mut conn).await?;
        assert_eq!(res, MbtType::FlatWithHash);

        let (mut conn, mbt) = open("../tests/fixtures/mbtiles/geography-class-jpg.mbtiles").await?;
        let res = mbt.detect_type(&mut conn).await?;
        assert_eq!(res, MbtType::Normalized);

        let (mut conn, mbt) = open(":memory:").await?;
        let res = mbt.detect_type(&mut conn).await;
        assert!(matches!(res, Err(MbtError::InvalidDataFormat(_))));

        Ok(())
    }

    #[actix_rt::test]
    async fn validate_valid_file() -> MbtResult<()> {
        let (mut conn, mbt) = open("../tests/fixtures/mbtiles/zoomed_world_cities.mbtiles").await?;
        mbt.check_integrity(&mut conn, IntegrityCheckType::Quick)
            .await?;
        Ok(())
    }

    #[actix_rt::test]
    async fn validate_invalid_file() -> MbtResult<()> {
        let (mut conn, mbt) =
            open("../tests/fixtures/files/invalid_zoomed_world_cities.mbtiles").await?;
        let result = mbt.check_agg_tiles_hashes(&mut conn).await;
        assert!(matches!(result, Err(MbtError::AggHashMismatch(..))));
        Ok(())
    }
}

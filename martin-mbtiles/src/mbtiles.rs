#![allow(clippy::missing_errors_doc)]

extern crate core;

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fmt::Display;
use std::path::Path;
use std::str::FromStr;

#[cfg(feature = "cli")]
use clap::ValueEnum;
use futures::TryStreamExt;
use log::{debug, info, warn};
use martin_tile_utils::{Format, TileInfo};
use serde_json::{Value as JSONValue, Value};
use sqlite_hashes::register_md5_function;
use sqlite_hashes::rusqlite::Connection as RusqliteConnection;
use sqlx::sqlite::SqliteRow;
use sqlx::{query, Row, SqliteExecutor};
use tilejson::{tilejson, Bounds, Center, TileJSON};

use crate::errors::{MbtError, MbtResult};
use crate::mbtiles_queries::{
    is_flat_tables_type, is_flat_with_hash_tables_type, is_normalized_tables_type,
};
use crate::MbtError::{FailedIntegrityCheck, GlobalHashValueNotFound, InvalidTileData};

#[derive(Clone, Debug, PartialEq)]
pub struct Metadata {
    pub id: String,
    pub tile_info: TileInfo,
    pub layer_type: Option<String>,
    pub tilejson: TileJSON,
    pub json: Option<JSONValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "cli", derive(ValueEnum))]
pub enum MbtType {
    Flat,
    FlatWithHash,
    Normalized,
}

#[derive(PartialEq, Eq, Default, Debug, Clone)]
#[cfg_attr(feature = "cli", derive(ValueEnum))]
pub enum IntegrityCheck {
    Full,
    #[default]
    Quick,
    Off,
}

#[derive(Clone, Debug)]
pub struct Mbtiles {
    filepath: String,
    filename: String,
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

    pub fn filepath(&self) -> &str {
        &self.filepath
    }

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

    pub async fn detect_type<T>(&self, conn: &mut T) -> MbtResult<MbtType>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let mbt_type = if is_normalized_tables_type(&mut *conn).await? {
            MbtType::Normalized
        } else if is_flat_with_hash_tables_type(&mut *conn).await? {
            MbtType::FlatWithHash
        } else if is_flat_tables_type(&mut *conn).await? {
            MbtType::Flat
        } else {
            return Err(MbtError::InvalidDataFormat(self.filepath.clone()));
        };

        self.check_for_uniqueness_constraint(&mut *conn, &mbt_type)
            .await?;

        Ok(mbt_type)
    }

    async fn check_for_uniqueness_constraint<T>(
        &self,
        conn: &mut T,
        mbt_type: &MbtType,
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

    async fn get_global_hash<T>(&self, conn: &mut T) -> MbtResult<Option<String>>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let rusqlite_conn = RusqliteConnection::open(Path::new(&self.filepath()))?;
        register_md5_function(&rusqlite_conn)?;
        let mbttype = self.detect_type(&mut *conn).await?;

        let sql = match mbttype {
            MbtType::Flat => {
                println!("Cannot generate global hash, no hash column in flat table format. Skipping global_hash generation...");
                return Ok(None);
            }
            MbtType::FlatWithHash => "SELECT hex(md5_concat(cast(zoom_level AS text), cast(tile_column AS text), cast(tile_row AS text), tile_hash)) FROM tiles_with_hash ORDER BY zoom_level, tile_column, tile_row;",
            MbtType::Normalized => "SELECT hex(md5_concat(cast(zoom_level AS text), cast(tile_column AS text), cast(tile_row AS text), tile_id)) FROM map ORDER BY zoom_level, tile_column, tile_row;"
        };

        Ok(Some(rusqlite_conn.query_row_and_then(sql, [], |row| {
            row.get::<_, String>(0)
        })?))
    }

    pub async fn generate_global_hash<T>(&self, conn: &mut T) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        if let Some(global_hash) = self.get_global_hash(&mut *conn).await? {
            self.set_metadata_value(conn, "global_hash", Some(global_hash))
                .await
        } else {
            Ok(())
        }
    }

    pub async fn validate_mbtiles<T>(
        &self,
        integrity_check: IntegrityCheck,
        conn: &mut T,
    ) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        // SQLite Integrity check
        if integrity_check != IntegrityCheck::Off {
            let sql = if integrity_check == IntegrityCheck::Full {
                "PRAGMA integrity_check;"
            } else {
                "PRAGMA quick_check;"
            };

            let result = query(sql)
                .map(|row: SqliteRow| row.get::<String, _>(0))
                .fetch_all(&mut *conn)
                .await?;

            if result.len() > 1
                || result.get(0).ok_or(FailedIntegrityCheck(
                    self.filepath().to_string(),
                    vec!["SQLite could not perform integrity check".to_string()],
                ))? != "ok"
            {
                return Err(FailedIntegrityCheck(self.filepath().to_string(), result));
            }
        }

        let mbttype = self.detect_type(&mut *conn).await?;

        if mbttype == MbtType::Flat {
            println!(
                "No hash column in flat table format, skipping hash-based validation steps..."
            );
            return Ok(());
        }

        let rusqlite_conn = RusqliteConnection::open(Path::new(self.filepath()))?;
        register_md5_function(&rusqlite_conn)?;

        // Global hash check
        if let Some(global_hash) = self.get_metadata_value(&mut *conn, "global_hash").await? {
            if let Some(new_global_hash) = self.get_global_hash(&mut *conn).await? {
                if global_hash != new_global_hash {
                    return Err(InvalidTileData(self.filepath().to_string()));
                }
            }
        } else {
            return Err(GlobalHashValueNotFound(self.filepath().to_string()));
        }

        // Per-tile hash check
        let sql = if mbttype == MbtType::FlatWithHash {
            "SELECT 1 FROM tiles_with_hash WHERE tile_hash != hex(md5(tile_data)) LIMIT 1;"
        } else {
            "SELECT 1 FROM images WHERE tile_id != hex(md5(tile_data)) LIMIT 1;"
        };

        if rusqlite_conn.prepare(sql)?.exists(())? {
            return Err(InvalidTileData(self.filepath().to_string()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use martin_tile_utils::Encoding;
    use sqlx::{Connection, SqliteConnection};
    use tilejson::VectorLayer;

    use super::*;

    async fn open(filepath: &str) -> (SqliteConnection, Mbtiles) {
        let mbt = Mbtiles::new(filepath).unwrap();
        (
            SqliteConnection::connect(mbt.filepath()).await.unwrap(),
            mbt,
        )
    }

    #[actix_rt::test]
    async fn mbtiles_meta() {
        let filepath = "../tests/fixtures/files/geography-class-jpg.mbtiles";
        let mbt = Mbtiles::new(filepath).unwrap();
        assert_eq!(mbt.filepath(), filepath);
        assert_eq!(mbt.filename(), "geography-class-jpg");
    }

    #[actix_rt::test]
    async fn metadata_jpeg() {
        let (mut conn, mbt) = open("../tests/fixtures/files/geography-class-jpg.mbtiles").await;
        let metadata = mbt.get_metadata(&mut conn).await.unwrap();
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
    }

    #[actix_rt::test]
    async fn metadata_mvt() {
        let (mut conn, mbt) = open("../tests/fixtures/files/world_cities.mbtiles").await;
        let metadata = mbt.get_metadata(&mut conn).await.unwrap();
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
    }

    #[actix_rt::test]
    async fn metadata_get_key() {
        let (mut conn, mbt) = open("../tests/fixtures/files/world_cities.mbtiles").await;

        let res = mbt.get_metadata_value(&mut conn, "bounds").await.unwrap();
        assert_eq!(res.unwrap(), "-123.123590,-37.818085,174.763027,59.352706");
        let res = mbt.get_metadata_value(&mut conn, "name").await.unwrap();
        assert_eq!(res.unwrap(), "Major cities from Natural Earth data");
        let res = mbt.get_metadata_value(&mut conn, "maxzoom").await.unwrap();
        assert_eq!(res.unwrap(), "6");
        let res = mbt.get_metadata_value(&mut conn, "nonexistent_key").await;
        assert_eq!(res.unwrap(), None);
        let res = mbt.get_metadata_value(&mut conn, "").await;
        assert_eq!(res.unwrap(), None);
    }

    #[actix_rt::test]
    async fn metadata_set_key() {
        let (mut conn, mbt) = open("file:metadata_set_key_mem_db?mode=memory&cache=shared").await;

        query("CREATE TABLE metadata (name text NOT NULL PRIMARY KEY, value text);")
            .execute(&mut conn)
            .await
            .unwrap();

        mbt.set_metadata_value(&mut conn, "bounds", Some("0.0, 0.0, 0.0, 0.0".to_string()))
            .await
            .unwrap();
        assert_eq!(
            mbt.get_metadata_value(&mut conn, "bounds")
                .await
                .unwrap()
                .unwrap(),
            "0.0, 0.0, 0.0, 0.0"
        );

        mbt.set_metadata_value(
            &mut conn,
            "bounds",
            Some("-123.123590,-37.818085,174.763027,59.352706".to_string()),
        )
        .await
        .unwrap();
        assert_eq!(
            mbt.get_metadata_value(&mut conn, "bounds")
                .await
                .unwrap()
                .unwrap(),
            "-123.123590,-37.818085,174.763027,59.352706"
        );

        mbt.set_metadata_value(&mut conn, "bounds", None)
            .await
            .unwrap();
        assert_eq!(
            mbt.get_metadata_value(&mut conn, "bounds").await.unwrap(),
            None
        );
    }

    #[actix_rt::test]
    async fn detect_type() {
        let (mut conn, mbt) = open("../tests/fixtures/files/world_cities.mbtiles").await;
        let res = mbt.detect_type(&mut conn).await.unwrap();
        assert_eq!(res, MbtType::Flat);

        let (mut conn, mbt) = open("../tests/fixtures/files/zoomed_world_cities.mbtiles").await;
        let res = mbt.detect_type(&mut conn).await.unwrap();
        assert_eq!(res, MbtType::FlatWithHash);

        let (mut conn, mbt) = open("../tests/fixtures/files/geography-class-jpg.mbtiles").await;
        let res = mbt.detect_type(&mut conn).await.unwrap();
        assert_eq!(res, MbtType::Normalized);

        let (mut conn, mbt) = open(":memory:").await;
        let res = mbt.detect_type(&mut conn).await;
        assert!(matches!(res, Err(MbtError::InvalidDataFormat(_))));
    }

    #[actix_rt::test]
    async fn validate_valid_file() {
        let (mut conn, mbt) = open("../tests/fixtures/files/zoomed_world_cities.mbtiles").await;

        mbt.validate_mbtiles(IntegrityCheck::Quick, &mut conn)
            .await
            .unwrap();
    }

    #[actix_rt::test]
    async fn validate_invalid_file() {
        let (mut conn, mbt) =
            open("../tests/fixtures/files/invalid_zoomed_world_cities.mbtiles").await;

        print!(
            "VLAUE {:?}",
            mbt.validate_mbtiles(IntegrityCheck::Quick, &mut conn).await
        );
        assert!(matches!(
            mbt.validate_mbtiles(IntegrityCheck::Quick, &mut conn)
                .await
                .unwrap_err(),
            MbtError::InvalidTileData(..)
        ));
    }
}

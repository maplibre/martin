#![allow(clippy::missing_errors_doc)]

extern crate core;

use futures::TryStreamExt;
use log::{debug, warn};
use martin_tile_utils::DataFormat;
use serde_json::{Value as JSONValue, Value};
use sqlx::pool::PoolConnection;
use sqlx::sqlite::SqlitePool;
use sqlx::{query, Pool, Sqlite};
use std::ffi::OsStr;
use std::fmt::Display;
use std::path::Path;
use std::str::FromStr;
use tilejson::{tilejson, Bounds, Center, TileJSON};

#[derive(thiserror::Error, Debug)]
pub enum MbtError {
    #[error("SQL Error {0}")]
    SqlError(#[from] sqlx::Error),

    #[error(r"Inconsistent tile formats detected: {0:?} vs {1:?}")]
    InconsistentMetadata(DataFormat, DataFormat),

    #[error("No tiles found")]
    NoTilesFound,
}

type MbtResult<T> = Result<T, MbtError>;

#[derive(Clone, Debug)]
pub struct Mbtiles {
    filename: String,
    pool: Pool<Sqlite>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Metadata {
    pub tilejson: TileJSON,
    pub id: String,
    pub tile_format: DataFormat,
    pub grid_format: Option<DataFormat>,
    pub layer_type: Option<String>,
    pub json: Option<JSONValue>,
}

impl Mbtiles {
    pub async fn new(file: &Path) -> MbtResult<Self> {
        // TODO: introduce a new error type for invalid file, instead of using lossy
        let pool = SqlitePool::connect(&file.to_string_lossy()).await?;
        let filename = file
            .file_stem()
            .unwrap_or_else(|| OsStr::new("unknown"))
            .to_string_lossy()
            .to_string();
        Ok(Self { filename, pool })
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

    pub async fn get_metadata(&self) -> MbtResult<Metadata> {
        let mut res = Metadata {
            tilejson: tilejson! {
                tiles: vec![String::new()],
            },
            id: self.filename.to_string(),
            tile_format: DataFormat::Unknown,
            grid_format: None, // TODO: get_grid_info(self.name, &connection),
            layer_type: None,
            json: None,
        };

        let mut conn = self.pool.acquire().await?;
        self.parse_metadata(&mut res, &mut conn).await?;
        self.detect_format(&mut res, &mut conn).await?;

        Ok(res)
    }

    async fn parse_metadata(
        &self,
        res: &mut Metadata,
        conn: &mut PoolConnection<Sqlite>,
    ) -> MbtResult<()> {
        let query = query!("SELECT name, value FROM metadata WHERE value IS NOT ''");
        let mut rows = query.fetch(conn);
        let tj = &mut res.tilejson;
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
                    "type" => res.layer_type = Some(value),
                    "legend" => tj.legend = Some(value),
                    "template" => tj.template = Some(value),
                    "json" => res.json = self.to_val(serde_json::from_str(&value), &name),
                    "format" | "generator" => {
                        tj.other.insert(name, Value::String(value));
                    }
                    _ => {
                        let file = &self.filename;
                        warn!("{file} has an unrecognized metadata value {name}={value}");
                        tj.other.insert(name, Value::String(value));
                    }
                }
            }
        }

        if let Some(JSONValue::Object(obj)) = &mut res.json {
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

        Ok(())
    }

    async fn detect_format(
        &self,
        meta: &mut Metadata,
        conn: &mut PoolConnection<Sqlite>,
    ) -> MbtResult<()> {
        let mut format = None;
        let mut tested_zoom = -1_i64;

        // First, pick any random tile
        let query = query! {"SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles WHERE zoom_level >= 0 LIMIT 1"};
        let row = query.fetch_optional(&mut *conn).await?;
        if let Some(r) = row {
            format = self.parse_tile(r.zoom_level, r.tile_column, r.tile_row, r.tile_data);
            tested_zoom = r.zoom_level.unwrap_or(-1);
        }

        // Afterwards, iterate over tiles in all allowed zooms and check for consistency
        for z in meta.tilejson.minzoom.unwrap_or(0)..=meta.tilejson.maxzoom.unwrap_or(18) {
            if i64::from(z) == tested_zoom {
                continue;
            }
            let query = query! {"SELECT tile_column, tile_row, tile_data FROM tiles WHERE zoom_level = ? LIMIT 1", z};
            let row = query.fetch_optional(&mut *conn).await?;
            if let Some(r) = row {
                match (
                    format,
                    self.parse_tile(Some(z.into()), r.tile_column, r.tile_row, r.tile_data),
                ) {
                    (_, None) => {}
                    (None | Some(DataFormat::Unknown), new) => format = new,
                    (Some(_), Some(DataFormat::Unknown)) => {}
                    (Some(old), Some(new)) if old == new => {}
                    (Some(old), Some(new)) => {
                        return Err(MbtError::InconsistentMetadata(old, new));
                    }
                }
            }
        }

        if let Some(Value::String(tj_fmt)) = meta.tilejson.other.get("format") {
            let fmt = match tj_fmt.to_ascii_lowercase().as_str() {
                "pbf" | "mvt" => DataFormat::Mvt,
                "jpg" | "jpeg" => DataFormat::Jpeg,
                "png" => DataFormat::Png,
                "gif" => DataFormat::Gif,
                "webp" => DataFormat::Webp,
                _ => {
                    warn!("Unknown format value in metadata: {tj_fmt}");
                    DataFormat::Unknown
                }
            };
            match (format, fmt) {
                (_, DataFormat::Unknown) => {}
                (None | Some(DataFormat::Unknown), new) => {
                    warn!("Unable to detect tile format, will use metadata.format '{new:?}' for file {}", self.filename);
                    format = Some(new);
                }
                (Some(old), new) if old == new || (old.is_mvt() && new.is_mvt()) => {
                    debug!("Detected tile format {old:?} matches metadata.format '{tj_fmt}' in file {}", self.filename);
                }
                (Some(old), _) => {
                    warn!("Found inconsistency: metadata.format='{tj_fmt}', but tiles were detected as {old:?} in file {}. Tiles will be returned as {old:?}.", self.filename);
                }
            }
        }

        if let Some(format) = format {
            if !format.is_mvt()
                && format != DataFormat::Unknown
                && meta.tilejson.vector_layers.is_some()
            {
                warn!(
                    "{} has vector_layers metadata but non-vector tiles",
                    self.filename
                );
            }
            meta.tile_format = format;
            Ok(())
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
    ) -> Option<DataFormat> {
        if let (Some(z), Some(x), Some(y), Some(tile)) = (z, x, y, tile) {
            let format = DataFormat::detect(&tile);
            debug!(
                "Tile {z}/{x}/{} is detected as {format:?} in file {}",
                (1 << z) - 1 - y,
                self.filename,
            );
            Some(format)
        } else {
            None
        }
    }

    pub async fn get_tile(&self, z: u8, x: u32, y: u32) -> MbtResult<Option<Vec<u8>>> {
        let mut conn = self.pool.acquire().await?;
        let y = (1 << z) - 1 - y;
        let query = query! {"SELECT tile_data from tiles where zoom_level = ? AND tile_column = ? AND tile_row = ?", z, x, y};
        let row = query.fetch_optional(&mut conn).await?;
        if let Some(row) = row {
            if let Some(tile_data) = row.tile_data {
                return Ok(Some(tile_data));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tilejson::VectorLayer;

    #[actix_rt::test]
    async fn test_metadata_jpeg() {
        let mbt = Mbtiles::new(Path::new("fixtures/geography-class-jpg.mbtiles")).await;
        let mbt = mbt.unwrap();
        let metadata = mbt.get_metadata().await.unwrap();
        let tj = metadata.tilejson;

        assert_eq!(tj.description.unwrap(), "One of the example maps that comes with TileMill - a bright & colorful world map that blends retro and high-tech with its folded paper texture and interactive flag tooltips. ");
        assert!(tj.legend.unwrap().starts_with("<div style="));
        assert_eq!(tj.maxzoom.unwrap(), 1);
        assert_eq!(tj.minzoom.unwrap(), 0);
        assert_eq!(tj.name.unwrap(), "Geography Class");
        assert_eq!(tj.template.unwrap(),"{{#__location__}}{{/__location__}}{{#__teaser__}}<div style=\"text-align:center;\">\n\n<img src=\"data:image/png;base64,{{flag_png}}\" style=\"-moz-box-shadow:0px 1px 3px #222;-webkit-box-shadow:0px 1px 5px #222;box-shadow:0px 1px 3px #222;\"><br>\n<strong>{{admin}}</strong>\n\n</div>{{/__teaser__}}{{#__full__}}{{/__full__}}");
        assert_eq!(tj.version.unwrap(), "1.0.0");
        assert_eq!(metadata.id, "geography-class-jpg");
        assert_eq!(metadata.tile_format, DataFormat::Jpeg);
    }

    #[actix_rt::test]
    async fn test_metadata_mvt() {
        let mbt = Mbtiles::new(Path::new("fixtures/world_cities.mbtiles")).await;
        let mbt = mbt.unwrap();
        let metadata = mbt.get_metadata().await.unwrap();
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
        assert_eq!(metadata.tile_format, DataFormat::GzipMvt);
        assert_eq!(metadata.layer_type, Some("overlay".to_string()));
    }
}

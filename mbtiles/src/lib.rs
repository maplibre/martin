#![allow(clippy::missing_errors_doc)]

extern crate core;

use futures::TryStreamExt;
use log::warn;
use martin_tile_utils::DataFormat;
use serde_json::Value as JSONValue;
use sqlx::sqlite::SqlitePool;
use sqlx::{query, Pool, Sqlite};
use std::ffi::OsStr;
use std::fmt::Display;
use std::path::Path;
use std::str::FromStr;
use tilejson::{tilejson, Bounds, Center, TileJSON};

type SqlResult<T> = Result<T, sqlx::Error>;

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
    pub async fn new(file: &Path) -> SqlResult<Self> {
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

    pub async fn get_metadata(&self) -> SqlResult<Metadata> {
        let mut res = Metadata {
            tilejson: tilejson! {
                tiles: vec![String::new()],
            },
            id: self.filename.to_string(),
            tile_format: DataFormat::Unknown, // TODO: compute
            grid_format: None,                // TODO: get_grid_info(self.name, &connection),
            layer_type: None,
            json: None,
        };

        let mut conn = self.pool.acquire().await?;
        let query = query!("SELECT name, value FROM metadata WHERE value IS NOT ''");
        let mut rows = query.fetch(&mut conn);

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
                    _ => {
                        let name = &self.filename;
                        warn!("{name} has an unrecognized metadata value {name}={value}");
                    }
                }
            }
        }
        Ok(res)
    }

    pub async fn get_tile(&self, z: u8, x: u32, y: u32) -> SqlResult<Option<Vec<u8>>> {
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

    #[actix_rt::test]
    async fn test_metadata() {
        let mbt = Mbtiles::new(Path::new("data/geography-class-jpg.mbtiles")).await;
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
        assert_eq!(metadata.tile_format, DataFormat::Unknown);
    }
}

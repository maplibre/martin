use std::fmt::Display;
use std::str::FromStr;

use futures::TryStreamExt;
use log::{info, warn};
use martin_tile_utils::TileInfo;
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use serde_json::{Value as JSONValue, Value, json};
use sqlx::{SqliteExecutor, query};
use tilejson::{Bounds, Center, TileJSON, tilejson};

use crate::MbtError::InvalidZoomValue;
use crate::Mbtiles;
use crate::errors::MbtResult;

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Metadata {
    pub id: String,
    #[serde(serialize_with = "serialize_ti")]
    pub tile_info: TileInfo,
    pub layer_type: Option<String>,
    pub tilejson: TileJSON,
    pub json: Option<JSONValue>,
    pub agg_tiles_hash: Option<String>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn serialize_ti<S: Serializer>(ti: &TileInfo, serializer: S) -> Result<S::Ok, S::Error> {
    let mut s = serializer.serialize_struct("TileInfo", 2)?;
    s.serialize_field("format", &ti.format.to_string())?;
    s.serialize_field(
        "encoding",
        ti.encoding.content_encoding().unwrap_or_default(),
    )?;
    s.end()
}

impl Mbtiles {
    fn to_val<V, E: Display>(&self, val: Result<V, E>, title: &str) -> Option<V> {
        match val {
            Ok(v) => Some(v),
            Err(err) => {
                let name = &self.filename();
                warn!("Unable to parse metadata {title} value in {name}: {err}");
                None
            }
        }
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

    pub async fn get_metadata_zoom_value<T>(
        &self,
        conn: &mut T,
        zoom_name: &'static str,
    ) -> MbtResult<Option<u8>>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        self.get_metadata_value(conn, zoom_name)
            .await?
            .map(|v| v.parse().map_err(|_| InvalidZoomValue(zoom_name, v)))
            .transpose()
    }

    pub async fn set_metadata_value<T, S>(&self, conn: &mut T, key: &str, value: S) -> MbtResult<()>
    where
        S: ToString,
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let value = value.to_string();
        query!(
            "INSERT OR REPLACE INTO metadata(name, value) VALUES(?, ?)",
            key,
            value
        )
        .execute(conn)
        .await?;
        Ok(())
    }

    pub async fn delete_metadata_value<T>(&self, conn: &mut T, key: &str) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        query!("DELETE FROM metadata WHERE name=?", key)
            .execute(conn)
            .await?;
        Ok(())
    }

    pub async fn get_metadata<T>(&self, conn: &mut T) -> MbtResult<Metadata>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        let query = query!("SELECT name, value FROM metadata WHERE value IS NOT ''");
        let mut rows = query.fetch(&mut *conn);

        let mut tj = tilejson! { tiles: vec![] };
        let mut layer_type: Option<String> = None;
        let mut json: Option<JSONValue> = None;
        let mut agg_tiles_hash: Option<String> = None;

        while let Some(row) = rows.try_next().await? {
            if let (Some(name), Some(value)) = (row.name, row.value) {
                match name.as_ref() {
                    // This list should loosely match the `insert_metadata` function below
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
                    "agg_tiles_hash" => agg_tiles_hash = Some(value),
                    "scheme" => {
                        if value != "tms" {
                            let file = &self.filename();
                            warn!(
                                "File {file} has an unexpected metadata value {name}='{value}'. Only 'tms' is supported. Ignoring."
                            );
                        }
                    }
                    _ => {
                        let file = &self.filename();
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
                        self.filename()
                    );
                }
            }
            if obj.is_empty() {
                json = None;
            }
        }

        // Need to drop rows in order to re-borrow connection reference as mutable
        drop(rows);

        Ok(Metadata {
            id: self.filename().to_string(),
            tile_info: self.detect_format(&tj, &mut *conn).await?,
            tilejson: tj,
            layer_type,
            json,
            agg_tiles_hash,
        })
    }

    pub async fn insert_metadata<T>(&self, conn: &mut T, tile_json: &TileJSON) -> MbtResult<()>
    where
        for<'e> &'e mut T: SqliteExecutor<'e>,
    {
        for (key, value) in &tile_json.other {
            if let Some(value) = value.as_str() {
                self.set_metadata_value(conn, key, value).await?;
            } else {
                self.set_metadata_value(conn, key, &serde_json::to_string(value)?)
                    .await?;
            }
        }
        for (key, value) in &[
            ("name", tile_json.name.as_deref()),
            ("version", tile_json.version.as_deref()),
            ("description", tile_json.description.as_deref()),
            ("attribution", tile_json.attribution.as_deref()),
            ("legend", tile_json.legend.as_deref()),
            ("template", tile_json.template.as_deref()),
        ] {
            if let Some(value) = value {
                self.set_metadata_value(conn, key, value).await?;
            }
        }
        if let Some(bounds) = &tile_json.bounds {
            self.set_metadata_value(conn, "bounds", bounds).await?;
        }
        if let Some(center) = &tile_json.center {
            self.set_metadata_value(conn, "center", center).await?;
        }
        if let Some(minzoom) = &tile_json.minzoom {
            self.set_metadata_value(conn, "minzoom", minzoom).await?;
        }
        if let Some(maxzoom) = &tile_json.maxzoom {
            self.set_metadata_value(conn, "maxzoom", maxzoom).await?;
        }
        if let Some(vector_layers) = &tile_json.vector_layers {
            self.set_metadata_value(
                conn,
                "json",
                &serde_json::to_string(&json!({ "vector_layers": vector_layers }))?,
            )
            .await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use martin_tile_utils::{Encoding, Format};
    use sqlx::Executor as _;
    use tilejson::VectorLayer;

    use super::*;
    use crate::mbtiles::tests::open;

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

        assert_eq!(
            tj.description.unwrap(),
            "One of the example maps that comes with TileMill - a bright & colorful world map that blends retro and high-tech with its folded paper texture and interactive flag tooltips. "
        );
        assert!(tj.legend.unwrap().starts_with("<div style="));
        assert_eq!(tj.maxzoom.unwrap(), 1);
        assert_eq!(tj.minzoom.unwrap(), 0);
        assert_eq!(tj.name.unwrap(), "Geography Class");
        assert_eq!(
            tj.template.unwrap(),
            "{{#__location__}}{{/__location__}}{{#__teaser__}}<div style=\"text-align:center;\">\n\n<img src=\"data:image/png;base64,{{flag_png}}\" style=\"-moz-box-shadow:0px 1px 3px #222;-webkit-box-shadow:0px 1px 5px #222;box-shadow:0px 1px 3px #222;\"><br>\n<strong>{{admin}}</strong>\n\n</div>{{/__teaser__}}{{#__full__}}{{/__full__}}"
        );
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
                other: BTreeMap::default()
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

        mbt.set_metadata_value(&mut conn, "bounds", "0.0, 0.0, 0.0, 0.0")
            .await?;
        assert_eq!(
            mbt.get_metadata_value(&mut conn, "bounds").await?.unwrap(),
            "0.0, 0.0, 0.0, 0.0"
        );

        mbt.set_metadata_value(
            &mut conn,
            "bounds",
            "-123.123590,-37.818085,174.763027,59.352706",
        )
        .await?;
        assert_eq!(
            mbt.get_metadata_value(&mut conn, "bounds").await?.unwrap(),
            "-123.123590,-37.818085,174.763027,59.352706"
        );

        mbt.delete_metadata_value(&mut conn, "bounds").await?;
        assert_eq!(mbt.get_metadata_value(&mut conn, "bounds").await?, None);

        Ok(())
    }
}

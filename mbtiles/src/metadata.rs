use std::fmt::Display;
use std::str::FromStr;

use futures::TryStreamExt;
use log::{info, warn};
use martin_tile_utils::TileInfo;
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use serde_json::{Value as JSONValue, Value};
use sqlx::{query, SqliteExecutor};
use tilejson::{tilejson, Bounds, Center, TileJSON};

use crate::errors::MbtResult;
use crate::Mbtiles;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Metadata {
    pub id: String,
    #[serde(serialize_with = "serialize_ti")]
    pub tile_info: TileInfo,
    pub layer_type: Option<String>,
    pub tilejson: TileJSON,
    pub json: Option<JSONValue>,
}

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

    pub async fn set_metadata_value<T>(
        &self,
        conn: &mut T,
        key: &str,
        value: Option<&str>,
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
            id: self.filename().to_string(),
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
        }

        Ok((tj, layer_type, json))
    }

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
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

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

        mbt.set_metadata_value(&mut conn, "bounds", Some("0.0, 0.0, 0.0, 0.0"))
            .await?;
        assert_eq!(
            mbt.get_metadata_value(&mut conn, "bounds").await?.unwrap(),
            "0.0, 0.0, 0.0, 0.0"
        );

        mbt.set_metadata_value(
            &mut conn,
            "bounds",
            Some("-123.123590,-37.818085,174.763027,59.352706"),
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
}

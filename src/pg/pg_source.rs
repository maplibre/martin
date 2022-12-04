use crate::pg::pool::Pool;
use crate::pg::utils::{io_error, is_valid_zoom, query_to_json};
use crate::source::{Source, Tile, UrlQuery, Xyz};
use async_trait::async_trait;
use bb8_postgres::tokio_postgres::types::ToSql;
use log::debug;
use martin_tile_utils::DataFormat;
use postgres::types::Type;
use std::collections::HashMap;
use std::io;
use tilejson::TileJSON;

#[derive(Clone, Debug)]
pub struct PgSource {
    id: String,
    info: PgSqlInfo,
    pool: Pool,
    tilejson: TileJSON,
}

impl PgSource {
    pub fn new(id: String, info: PgSqlInfo, tilejson: TileJSON, pool: Pool) -> Self {
        Self {
            tilejson,
            id,
            info,
            pool,
        }
    }
}

#[async_trait]
impl Source for PgSource {
    fn get_tilejson(&self) -> TileJSON {
        self.tilejson.clone()
    }

    fn get_format(&self) -> DataFormat {
        DataFormat::Mvt
    }

    fn clone_source(&self) -> Box<dyn Source> {
        Box::new(self.clone())
    }

    fn is_valid_zoom(&self, zoom: i32) -> bool {
        is_valid_zoom(zoom, self.tilejson.minzoom, self.tilejson.maxzoom)
    }

    async fn get_tile(&self, xyz: &Xyz, url_query: &Option<UrlQuery>) -> Result<Tile, io::Error> {
        let empty_query = HashMap::new();
        let url_query = url_query.as_ref().unwrap_or(&empty_query);
        let conn = self.pool.get().await?;

        let param_types: &[Type] = if self.info.has_query_params {
            &[Type::INT4, Type::INT4, Type::INT4, Type::JSON]
        } else {
            &[Type::INT4, Type::INT4, Type::INT4]
        };

        let query = &self.info.query;
        let prep_query = conn
            .prepare_typed(query, param_types)
            .await
            .map_err(|e| io_error!(e, "Can't create prepared statement for the tile"))?;

        let tile = if self.info.has_query_params {
            let json = query_to_json(url_query);
            debug!("SQL: {query} [{xyz:,>}, {json:?}]");
            let params: &[&(dyn ToSql + Sync)] = &[&xyz.z, &xyz.x, &xyz.y, &json];
            conn.query_one(&prep_query, params).await
        } else {
            debug!("SQL: {query} [{xyz:,>}]");
            conn.query_one(&prep_query, &[&xyz.z, &xyz.x, &xyz.y]).await
        };

        let tile = tile.map(|row| row.get(0)).map_err(|e| {
            if self.info.has_query_params {
                let url_q = query_to_json(url_query);
                io_error!(e, r#"Can't get {}/{xyz:/>} with {url_q:?} params"#, self.id)
            } else {
                io_error!(e, r#"Can't get {}/{xyz:/>}"#, self.id)
            }
        })?;

        Ok(tile)
    }
}

#[derive(Clone, Debug)]
pub struct PgSqlInfo {
    pub query: String,
    pub has_query_params: bool,
    pub signature: String,
}

impl PgSqlInfo {
    pub fn new(query: String, has_query_params: bool, signature: String) -> Self {
        Self {
            query,
            has_query_params,
            signature,
        }
    }
}

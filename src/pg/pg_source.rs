use std::collections::HashMap;

use async_trait::async_trait;
use bb8_postgres::tokio_postgres::types::ToSql;
use log::debug;
use martin_tile_utils::DataFormat;
use postgres::types::Type;
use tilejson::TileJSON;

use crate::pg::pool::Pool;
use crate::pg::utils::query_to_json;
use crate::pg::utils::PgError::{GetTileError, GetTileWithQueryError, PrepareQueryError};
use crate::source::{Source, Tile, UrlQuery, Xyz};
use crate::utils::{is_valid_zoom, Result};

#[derive(Clone, Debug)]
pub struct PgSource {
    id: String,
    info: PgSqlInfo,
    pool: Pool,
    tilejson: TileJSON,
}

impl PgSource {
    #[must_use]
    pub fn new(id: String, info: PgSqlInfo, tilejson: TileJSON, pool: Pool) -> Self {
        Self {
            id,
            info,
            pool,
            tilejson,
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

    fn support_url_query(&self) -> bool {
        self.info.use_url_query
    }

    async fn get_tile(&self, xyz: &Xyz, url_query: &Option<UrlQuery>) -> Result<Tile> {
        let empty_query = HashMap::new();
        let url_query = url_query.as_ref().unwrap_or(&empty_query);
        let conn = self.pool.get().await?;

        let param_types: &[Type] = if self.support_url_query() {
            &[Type::INT4, Type::INT4, Type::INT4, Type::JSON]
        } else {
            &[Type::INT4, Type::INT4, Type::INT4]
        };

        let query = &self.info.query;
        let prep_query = conn.prepare_typed(query, param_types).await.map_err(|e| {
            PrepareQueryError(
                e,
                self.id.to_string(),
                self.info.signature.to_string(),
                self.info.query.to_string(),
            )
        })?;

        let tile = if self.support_url_query() {
            let json = query_to_json(url_query);
            debug!("SQL: {query} [{xyz}, {json:?}]");
            let params: &[&(dyn ToSql + Sync)] = &[&xyz.z, &xyz.x, &xyz.y, &json];
            conn.query_opt(&prep_query, params).await
        } else {
            debug!("SQL: {query} [{xyz}]");
            conn.query_opt(&prep_query, &[&xyz.z, &xyz.x, &xyz.y]).await
        };

        let tile = tile
            .map(|row| row.and_then(|r| r.get::<_, Option<Tile>>(0)))
            .map_err(|e| {
                if self.support_url_query() {
                    GetTileWithQueryError(e, self.id.to_string(), *xyz, url_query.clone())
                } else {
                    GetTileError(e, self.id.to_string(), *xyz)
                }
            })?
            .unwrap_or_default();

        Ok(tile)
    }
}

#[derive(Clone, Debug)]
pub struct PgSqlInfo {
    pub query: String,
    pub use_url_query: bool,
    pub signature: String,
}

impl PgSqlInfo {
    #[must_use]
    pub fn new(query: String, has_query_params: bool, signature: String) -> Self {
        Self {
            query,
            use_url_query: has_query_params,
            signature,
        }
    }
}

use std::collections::HashMap;

use async_trait::async_trait;
use deadpool_postgres::tokio_postgres::types::{ToSql, Type};
use log::debug;
use martin_tile_utils::Encoding::Uncompressed;
use martin_tile_utils::Format::Mvt;
use martin_tile_utils::TileInfo;
use tilejson::TileJSON;

use crate::pg::pool::PgPool;
use crate::pg::utils::query_to_json;
use crate::pg::PgError::{GetTileError, GetTileWithQueryError, PrepareQueryError};
use crate::source::{Source, Tile, UrlQuery};
use crate::{Result, Xyz};

#[derive(Clone, Debug)]
pub struct PgSource {
    id: String,
    info: PgSqlInfo,
    pool: PgPool,
    tilejson: TileJSON,
}

impl PgSource {
    #[must_use]
    pub fn new(id: String, info: PgSqlInfo, tilejson: TileJSON, pool: PgPool) -> Self {
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
    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_tilejson(&self) -> &TileJSON {
        &self.tilejson
    }

    fn get_tile_info(&self) -> TileInfo {
        TileInfo::new(Mvt, Uncompressed)
    }

    fn clone_source(&self) -> Box<dyn Source> {
        Box::new(self.clone())
    }

    fn support_url_query(&self) -> bool {
        self.info.use_url_query
    }

    async fn get_tile(&self, xyz: &Xyz, url_query: &Option<UrlQuery>) -> Result<Tile> {
        let empty_query = HashMap::new();
        let url_query = url_query.as_ref().unwrap_or(&empty_query);
        let conn = self.pool.get().await?;

        let param_types: &[Type] = if self.support_url_query() {
            &[Type::INT2, Type::INT8, Type::INT8, Type::JSON]
        } else {
            &[Type::INT2, Type::INT8, Type::INT8]
        };

        let query = &self.info.query;
        let prep_query = conn
            .prepare_typed_cached(query, param_types)
            .await
            .map_err(|e| {
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
            let params: &[&(dyn ToSql + Sync)] = &[
                &i16::from(xyz.z),
                &i64::from(xyz.x),
                &i64::from(xyz.y),
                &json,
            ];
            conn.query_opt(&prep_query, params).await
        } else {
            debug!("SQL: {query} [{xyz}]");
            conn.query_opt(
                &prep_query,
                &[&i16::from(xyz.z), &i64::from(xyz.x), &i64::from(xyz.y)],
            )
            .await
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

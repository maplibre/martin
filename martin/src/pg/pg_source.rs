use async_trait::async_trait;
use deadpool_postgres::tokio_postgres::types::{ToSql, Type};
use log::debug;
use martin_tile_utils::Encoding::Uncompressed;
use martin_tile_utils::Format::Mvt;
use martin_tile_utils::{TileCoord, TileInfo};
use tilejson::TileJSON;

use crate::file_config::ValidationLevel;
use crate::MartinResult;
use crate::pg::PgError::{GetTileError, GetTileWithQueryError, PrepareQueryError};
use crate::pg::pool::PgPool;
use crate::pg::utils::query_to_json;
use crate::source::{Source, TileData, TileInfoSource, UrlQuery};

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

    fn clone_source(&self) -> TileInfoSource {
        Box::new(self.clone())
    }

    fn support_url_query(&self) -> bool {
        self.info.use_url_query
    }

    async fn validate(&self, _validation_level: ValidationLevel) -> MartinResult<()> {
        MartinResult::Ok(())
    }

    async fn get_tile(
        &self,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
    ) -> MartinResult<TileData> {
        let conn = self.pool.get().await?;
        let param_types: &[Type] = if self.support_url_query() {
            &[Type::INT2, Type::INT8, Type::INT8, Type::JSON]
        } else {
            &[Type::INT2, Type::INT8, Type::INT8]
        };

        let sql = &self.info.sql_query;
        let prep_query = conn
            .prepare_typed_cached(sql, param_types)
            .await
            .map_err(|e| {
                PrepareQueryError(
                    e,
                    self.id.to_string(),
                    self.info.signature.to_string(),
                    self.info.sql_query.to_string(),
                )
            })?;

        let tile = if self.support_url_query() {
            let json = query_to_json(url_query);
            debug!("SQL: {sql} [{xyz}, {json:?}]");
            let params: &[&(dyn ToSql + Sync)] = &[
                &i16::from(xyz.z),
                &i64::from(xyz.x),
                &i64::from(xyz.y),
                &json,
            ];
            conn.query_opt(&prep_query, params).await
        } else {
            debug!("SQL: {sql} [{xyz}]");
            conn.query_opt(
                &prep_query,
                &[&i16::from(xyz.z), &i64::from(xyz.x), &i64::from(xyz.y)],
            )
            .await
        };

        let tile = tile
            .map(|row| row.and_then(|r| r.get::<_, Option<TileData>>(0)))
            .map_err(|e| {
                if self.support_url_query() {
                    GetTileWithQueryError(e, self.id.to_string(), xyz, url_query.cloned())
                } else {
                    GetTileError(e, self.id.to_string(), xyz)
                }
            })?
            .unwrap_or_default();

        Ok(tile)
    }
}

#[derive(Clone, Debug)]
pub struct PgSqlInfo {
    pub sql_query: String,
    pub use_url_query: bool,
    pub signature: String,
}

impl PgSqlInfo {
    #[must_use]
    pub fn new(query: String, has_query_params: bool, signature: String) -> Self {
        Self {
            sql_query: query,
            use_url_query: has_query_params,
            signature,
        }
    }
}

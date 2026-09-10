use async_trait::async_trait;
use deadpool_postgres::tokio_postgres::Row;
use deadpool_postgres::tokio_postgres::types::{ToSql, Type};
use martin_tile_utils::{Encoding, TileCoord, TileData, TileGrid, TileInfo};
use tilejson::TileJSON;
use tracing::{debug, instrument};

use crate::CacheZoomRange;
use crate::tiles::postgres::PostgresError::{
    GetTileError, GetTileWithQueryError, PrepareQueryError,
};
use crate::tiles::postgres::utils::query_to_json;
use crate::tiles::postgres::{ActiveQueryRegistry, PostgresPool};
use crate::tiles::{BoxedSource, MartinCoreResult, Source, Tile, UrlQuery};

#[derive(Clone, Debug)]
/// `PostgreSQL` tile source that executes SQL queries to generate tiles.
pub struct PostgresSource {
    id: String,
    info: PostgresSqlInfo,
    pool: PostgresPool,
    tilejson: TileJSON,
    tile_info: TileInfo,
    cache_zoom: CacheZoomRange,
    tile_grid: TileGrid,
}

impl PostgresSource {
    /// Creates a new `PostgreSQL` tile source.
    #[must_use]
    pub const fn new(
        id: String,
        info: PostgresSqlInfo,
        tilejson: TileJSON,
        pool: PostgresPool,
        tile_info: TileInfo,
        cache_zoom: CacheZoomRange,
        tile_grid: TileGrid,
    ) -> Self {
        Self {
            id,
            info,
            pool,
            tilejson,
            tile_info,
            cache_zoom,
            tile_grid,
        }
    }
}

#[async_trait]
impl Source for PostgresSource {
    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_tilejson(&self) -> &TileJSON {
        &self.tilejson
    }

    fn get_tile_info(&self) -> TileInfo {
        self.tile_info
    }

    fn tile_grid(&self) -> &TileGrid {
        &self.tile_grid
    }

    fn clone_source(&self) -> BoxedSource {
        Box::new(self.clone())
    }

    fn support_url_query(&self) -> bool {
        self.info.use_url_query
    }

    fn benefits_from_concurrent_scraping(&self) -> bool {
        // pg does not parallelize queries well internally and having more requests in flight is thus beneficial
        true
    }

    fn empty_tile_implies_empty_children(&self) -> bool {
        self.info.empty_tile_implies_empty_children
    }

    fn cache_zoom(&self) -> CacheZoomRange {
        self.cache_zoom
    }

    fn cancel_registry(&self) -> Option<ActiveQueryRegistry> {
        Some(self.pool.active_query_registry().clone())
    }

    async fn get_tile(
        &self,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<TileData> {
        Ok(self
            .query_row(xyz, url_query)
            .await?
            .and_then(|row| row.get::<_, Option<TileData>>(0))
            .unwrap_or_default())
    }

    async fn get_tile_with_etag(
        &self,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<Tile> {
        if !self.sql_for(url_query).has_etag_column {
            let data = self.get_tile(xyz, url_query).await?;
            let info = self.tile_info_for(&data);
            return Ok(Tile::new_hash_etag(data, info));
        }
        let row = self.query_row(xyz, url_query).await?;
        let data: TileData = row
            .as_ref()
            .and_then(|row| row.get::<_, Option<TileData>>(0))
            .unwrap_or_default();
        let etag: Option<String> = row.and_then(|row| row.get::<_, Option<String>>(1));
        let info = self.tile_info_for(&data);
        match etag {
            Some(etag) if !data.is_empty() && !etag.is_empty() => {
                Ok(Tile::new_with_etag(data, info, etag))
            }
            _ => Ok(Tile::new_hash_etag(data, info)),
        }
    }
}

impl PostgresSource {
    /// The query answering a request, by whether the request carries a query string.
    fn sql_for(&self, url_query: Option<&UrlQuery>) -> &PostgresSqlInfo {
        match (&self.info.queryless, url_query) {
            (Some(queryless), None) => queryless,
            _ => &self.info,
        }
    }

    /// The declared tile info, with the encoding the bytes carry when the function compressed them.
    fn tile_info_for(&self, data: &[u8]) -> TileInfo {
        match Encoding::detect(data) {
            Some(encoding) => TileInfo::new(self.tile_info.format, encoding),
            None => self.tile_info,
        }
    }

    /// Runs the tile query, returning the row when the query produced one.
    #[instrument(
        level = "debug",
        skip_all,
        fields(
            source.id = %self.id,
            tile.z = xyz.z,
            tile.x = xyz.x,
            tile.y = xyz.y,
        ),
        err(Debug),
    )]
    async fn query_row(
        &self,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<Option<Row>> {
        let conn = self.pool.get().await?;

        let cancel_token = conn.cancel_token();

        // Auto-clean up if task completes or is interrupted
        let _query_guard = self.pool.active_query_registry().register(cancel_token);

        let info = self.sql_for(url_query);
        let param_types: &[Type] = if info.use_url_query {
            &[Type::INT2, Type::INT8, Type::INT8, Type::JSON]
        } else {
            &[Type::INT2, Type::INT8, Type::INT8]
        };

        let sql = &info.sql_query;
        let prep_query = conn
            .prepare_typed_cached(sql, param_types)
            .await
            .map_err(|e| PrepareQueryError {
                source: e,
                source_id: self.id.clone(),
                signature: info.signature.clone(),
                query: info.sql_query.clone(),
            })?;

        let tile = if info.use_url_query {
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

        Ok(tile.map_err(|e| {
            if info.use_url_query {
                GetTileWithQueryError(e, self.id.clone(), xyz, url_query.cloned())
            } else {
                GetTileError(e, self.id.clone(), xyz)
            }
        })?)
    }
}

#[derive(Clone, Debug)]
/// SQL query information for `PostgreSQL` tile sources.
pub struct PostgresSqlInfo {
    /// SQL query string.
    pub sql_query: String,
    /// Whether the query uses URL query parameters.
    pub use_url_query: bool,
    /// Whether an empty tile implies that all tiles below it are empty.
    pub empty_tile_implies_empty_children: bool,
    /// Signature of the query.
    pub signature: String,
    /// Whether the query's second column is the tile's `ETag`.
    pub has_etag_column: bool,
    /// The variant to run for a request without a query string, when the function has one.
    pub queryless: Option<Box<Self>>,
}

impl PostgresSqlInfo {
    /// Creates new SQL query information.
    #[must_use]
    pub const fn new(
        query: String,
        has_query_params: bool,
        empty_tile_implies_empty_children: bool,
        signature: String,
        has_etag_column: bool,
    ) -> Self {
        Self {
            sql_query: query,
            use_url_query: has_query_params,
            empty_tile_implies_empty_children,
            signature,
            has_etag_column,
            queryless: None,
        }
    }

    /// This query with `queryless` answering the requests that carry no query string.
    #[must_use]
    pub fn with_queryless(mut self, queryless: Self) -> Self {
        self.queryless = Some(Box::new(queryless));
        self
    }
}

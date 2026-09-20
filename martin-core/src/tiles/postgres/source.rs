use std::collections::HashMap;

use compact_str::CompactString;
use deadpool_postgres::Object;
use deadpool_postgres::tokio_postgres::types::{Json, ToSql, Type};
use deadpool_postgres::tokio_postgres::{Row, Statement};
use martin_tile_utils::{Encoding, TileCoord, TileData, TileGrid, TileInfo};
use tilejson::TileJSON;
use tracing::{debug, instrument};

use crate::CacheZoomRange;
use crate::tiles::postgres::PostgresError::{
    GetTileError, GetTileWithQueryError, PrepareQueryError,
};
use crate::tiles::postgres::features::features_from_rows;
use crate::tiles::postgres::pool::ActiveQueryGuard;
use crate::tiles::postgres::utils::query_to_json;
use crate::tiles::postgres::{
    ActiveQueryRegistry, PostgresError, PostgresPool, PostgresTileFeatures,
};
use crate::tiles::{MartinCoreResult, Source, Tile, UrlQuery};

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

impl PostgresSource {
    /// The registry tracking this pool's in-flight queries, so a dropped request
    /// can cancel the statement it started.
    #[must_use]
    pub fn active_query_registry(&self) -> ActiveQueryRegistry {
        self.pool.active_query_registry().clone()
    }

    /// The features of a tile instead of its serialized bytes.
    ///
    /// `None` means this source has no row-per-feature form, which is every source but a table.
    /// A caller that wants a tile format `PostGIS` does not produce itself can encode these
    /// instead of taking an `ST_AsMVT` tile apart again.
    pub async fn get_tile_features(
        &self,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<Option<PostgresTileFeatures>> {
        let info = self.sql_for(url_query);
        let Some(row_query) = &info.row_query else {
            return Ok(None);
        };
        let rows = self
            .query_feature_rows(info, row_query, xyz, url_query)
            .await?;
        Ok(Some(PostgresTileFeatures {
            layer_name: row_query.layer_name.clone(),
            extent: row_query.extent,
            features: features_from_rows(&rows, row_query.has_id_column)?,
        }))
    }
}

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

    async fn get_tile(
        &self,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<TileData> {
        Ok(self
            .query_row(xyz, url_query)
            .await?
            .and_then(|row| row.get::<_, Option<Vec<u8>>>(0))
            .map(TileData::from)
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
            .and_then(|row| row.get::<_, Option<Vec<u8>>>(0))
            .map(TileData::from)
            .unwrap_or_default();
        let etag = row
            .and_then(|row| row.get::<_, Option<String>>(1))
            .map(CompactString::from);
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
            tile.z = xyz.z(),
            tile.x = xyz.x(),
            tile.y = xyz.y(),
        ),
        err(Debug),
    )]
    async fn query_row(
        &self,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<Option<Row>> {
        let info = self.sql_for(url_query);
        let query = self
            .tile_query(info, &info.sql_query, xyz, url_query)
            .await?;
        let row = query
            .conn
            .query_opt(&query.statement, &query.params())
            .await;
        Ok(row.map_err(|e| self.run_error(e, xyz, url_query, info.use_url_query))?)
    }

    /// Runs the row-per-feature query, returning one row per feature of the tile.
    #[instrument(
        level = "debug",
        skip_all,
        fields(
            source.id = %self.id,
            tile.z = xyz.z(),
            tile.x = xyz.x(),
            tile.y = xyz.y(),
        ),
        err(Debug),
    )]
    async fn query_feature_rows(
        &self,
        info: &PostgresSqlInfo,
        row_query: &PostgresRowQuery,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<Vec<Row>> {
        let query = self
            .tile_query(info, &row_query.sql_query, xyz, url_query)
            .await?;
        let rows = query.conn.query(&query.statement, &query.params()).await;
        Ok(rows.map_err(|e| self.run_error(e, xyz, url_query, info.use_url_query))?)
    }

    /// Prepares one of this source's tile queries and binds `xyz` and the query string to it.
    ///
    /// All of them take `z/x/y` and an optional query string, and differ only in what they
    /// return, so every caller runs the statement itself.
    async fn tile_query(
        &self,
        info: &PostgresSqlInfo,
        sql: &str,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<TileQuery> {
        let conn = self.pool.get().await?;
        let guard = self
            .pool
            .active_query_registry()
            .register(conn.cancel_token());

        let param_types: &[Type] = if info.use_url_query {
            &[Type::INT2, Type::INT8, Type::INT8, Type::JSON]
        } else {
            &[Type::INT2, Type::INT8, Type::INT8]
        };
        let statement = conn
            .prepare_typed_cached(sql, param_types)
            .await
            .map_err(|e| PrepareQueryError {
                source: e,
                source_id: self.id.clone(),
                signature: info.signature.clone(),
                query: sql.to_owned(),
            })?;

        let url_query = if info.use_url_query {
            let json = query_to_json(url_query);
            debug!("SQL: {sql} [{xyz}, {json:?}]");
            Some(json)
        } else {
            debug!("SQL: {sql} [{xyz}]");
            None
        };

        Ok(TileQuery {
            conn,
            _cancel_guard: guard,
            statement,
            z: i16::from(xyz.z()),
            x: i64::from(xyz.x()),
            y: i64::from(xyz.y()),
            url_query,
        })
    }

    fn run_error(
        &self,
        e: deadpool_postgres::tokio_postgres::Error,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
        use_url_query: bool,
    ) -> PostgresError {
        if use_url_query {
            GetTileWithQueryError(e, self.id.clone(), xyz, url_query.cloned())
        } else {
            GetTileError(e, self.id.clone(), xyz)
        }
    }
}

/// A prepared tile query with its parameters bound, ready to run on `conn`.
///
/// It holds the registry guard, so the statement stays cancellable for exactly as long as the
/// query lives.
struct TileQuery {
    conn: Object,
    _cancel_guard: ActiveQueryGuard,
    statement: Statement,
    z: i16,
    x: i64,
    y: i64,
    url_query: Option<Json<HashMap<String, serde_json::Value>>>,
}

impl TileQuery {
    fn params(&self) -> Vec<&(dyn ToSql + Sync)> {
        let mut params: Vec<&(dyn ToSql + Sync)> = vec![&self.z, &self.x, &self.y];
        if let Some(url_query) = &self.url_query {
            params.push(url_query);
        }
        params
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
    /// The same tile as one row per feature, when the source can be read that way.
    pub row_query: Option<PostgresRowQuery>,
}

/// The row-per-feature form of a tile query, and what its rows make up.
///
/// Unlike [`PostgresSqlInfo::sql_query`] this hands out the features themselves rather than an
/// MVT blob, so a caller that wants another tile format does not have to take `ST_AsMVT`'s
/// output apart again. Only table sources have one: a function source returns a blob built by
/// user SQL, which has no row form.
#[derive(Clone, Debug)]
pub struct PostgresRowQuery {
    /// SQL taking `$1/$2/$3` as `z/x/y` and returning one row per feature, the geometry first.
    pub sql_query: String,
    /// Whether the second column is the feature id.
    pub has_id_column: bool,
    /// The name of the layer the features make up.
    pub layer_name: String,
    /// The tile extent the geometries are expressed in.
    pub extent: u32,
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
            row_query: None,
        }
    }

    /// This query with `queryless` answering the requests that carry no query string.
    #[must_use]
    pub fn with_queryless(mut self, queryless: Self) -> Self {
        self.queryless = Some(Box::new(queryless));
        self
    }

    /// This query with `row_query` as the row-per-feature form of the same tile.
    #[must_use]
    pub fn with_row_query(mut self, row_query: PostgresRowQuery) -> Self {
        self.row_query = Some(row_query);
        self
    }
}

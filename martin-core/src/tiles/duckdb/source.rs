use async_trait::async_trait;
use duckdb::{Connection, OptionalExt as _, params};
use martin_tile_utils::{TileCoord, TileData, TileInfo};
use tilejson::TileJSON;
use tracing::{instrument, trace};

use crate::CacheZoomRange;
use crate::tiles::duckdb::DuckDBError::{GetTileError, PrepareQueryError};
use crate::tiles::duckdb::{DuckDBPool, DuckDBResult};
use crate::tiles::{BoxedSource, MartinCoreResult, Source, UrlQuery};

#[derive(Clone, Debug)]
/// `DuckDB File` tile source that executes SQL queries to generate tiles.
pub struct DuckDBSource {
    id: String,
    info: DuckDBSqlInfo,
    pool: DuckDBPool,
    tilejson: TileJSON,
    tile_info: TileInfo,
    cache_zoom: CacheZoomRange,
}

impl DuckDBSource {
    /// Creates a new `DuckDBFile` tile source.
    #[must_use]
    pub fn new(
        id: String,
        info: DuckDBSqlInfo,
        tilejson: TileJSON,
        pool: DuckDBPool,
        tile_info: TileInfo,
        cache_zoom: CacheZoomRange,
    ) -> Self {
        Self {
            id,
            info,
            pool,
            tilejson,
            tile_info,
            cache_zoom,
        }
    }
}

#[async_trait]
impl Source for DuckDBSource {
    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_tilejson(&self) -> &TileJSON {
        &self.tilejson
    }

    fn get_tile_info(&self) -> TileInfo {
        self.tile_info
    }

    fn clone_source(&self) -> BoxedSource {
        Box::new(self.clone())
    }

    fn support_url_query(&self) -> bool {
        false
    }

    fn benefits_from_concurrent_scraping(&self) -> bool {
        //duckdb parallelizes queries internally on a fixed number of threads so having too many concurrent queries is not beneficial. but can be set to say 4 instead of 20+ for pg
        false
    }

    fn cache_zoom(&self) -> CacheZoomRange {
        self.cache_zoom
    }

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
    async fn get_tile(
        &self,
        xyz: TileCoord,
        _url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<TileData> {
        let id = self.id.clone();
        let info = self.info.clone();
        let tile = self
            .pool
            .generate_tile(move |conn| execute_tile_query(&id, &info, xyz, conn))
            .await?;

        Ok(tile)
    }
}

#[derive(Clone, Debug)]
/// SQL query information for `DuckDB` tile sources.
pub struct DuckDBSqlInfo {
    /// SQL query string.
    pub sql_query: String,
    /// Whether the query uses URL query parameters.
    pub use_url_query: bool,
    /// Signature of the query.
    pub signature: String,
}

impl DuckDBSqlInfo {
    /// Creates new SQL query information.
    #[must_use]
    pub fn new(query: String, has_query_params: bool, signature: String) -> Self {
        Self {
            sql_query: query,
            use_url_query: has_query_params,
            signature,
        }
    }
}

fn execute_tile_query(
    source_id: &str,
    info: &DuckDBSqlInfo,
    xyz: TileCoord,
    conn: &Connection,
) -> DuckDBResult<TileData> {
    let sql = &info.sql_query;
    let mut stmt = conn
        .prepare_cached(sql)
        .map_err(|source| PrepareQueryError {
            source: source.into(),
            source_id: source_id.to_string(),
            signature: info.signature.clone(),
            query: info.sql_query.clone(),
        })?;

    trace!(%sql, %xyz, "duckdb tile query");
    let tile = stmt
        .query_one(
            params![i16::from(xyz.z), i64::from(xyz.x), i64::from(xyz.y)],
            |row| row.get::<_, Option<TileData>>(0),
        )
        .optional()
        .map_err(|e| GetTileError(e.into(), source_id.to_string(), xyz))?
        .flatten()
        .unwrap_or_default();

    Ok(tile)
}

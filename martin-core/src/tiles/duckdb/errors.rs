//! Error types for `DuckDB` tile sources and pools.

use deadpool::managed::{BuildError, PoolError};
use duckdb::Error as DuckdbError;
use martin_tile_utils::TileCoord;
use std::path::PathBuf;
use tokio::task::JoinError;

use crate::tiles::UrlQuery;
use crate::tiles::duckdb::pool::DuckDBPoolTarget;

/// Result type for `DuckDB` operations.
pub type DuckDBResult<T> = Result<T, DuckDBError>;

/// Errors raised while creating pooled `DuckDB` connections.
#[derive(thiserror::Error, Debug)]
pub enum DuckDBPoolManagerError {
    /// The blocking task used for connection setup failed.
    #[error("DuckDB pool creation failed: {0}")]
    Join(#[from] JoinError),

    /// The `DuckDB` connection could not be opened.
    #[error("Unable to open DuckDB connection for {target}: {source}")]
    Open {
        /// Underlying `DuckDB` error.
        #[source]
        source: Box<DuckdbError>,
        /// Source target.
        target: Box<DuckDBPoolTarget>,
    },

    /// A required `DuckDB` extension could not be loaded.
    #[error("Unable to load DuckDB extension {extension} for {target}: {source}")]
    LoadExtension {
        /// Underlying `DuckDB` error.
        #[source]
        source: Box<DuckdbError>,
        /// Extension name.
        extension: &'static str,
        /// Source target.
        target: Box<DuckDBPoolTarget>,
    },

    /// A pool-wide session setting could not be applied.
    #[error("Unable to apply DuckDB setting {setting}={value} for {target}: {source}")]
    ApplySetting {
        /// Underlying `DuckDB` error.
        #[source]
        source: Box<DuckdbError>,
        /// Setting name.
        setting: &'static str,
        /// Setting value.
        value: String,
        /// Source target.
        target: Box<DuckDBPoolTarget>,
    },

    /// Thread count cannot be represented as a `DuckDB` setting value.
    #[error("Unable to apply DuckDB setting threads={0}: value is too large")]
    InvalidThreadCount(usize),

    /// A local GeoParquet path cannot be represented as UTF-8 for SQL usage.
    #[error("Unable to use GeoParquet path for {target}: non-UTF8 path {path:?}")]
    NonUtf8Path {
        /// The invalid path value.
        path: PathBuf,
        /// Source target.
        target: Box<DuckDBPoolTarget>,
    },

    /// The `DuckDB` connection failed a pool recycle health check.
    #[error("DuckDB connection health check failed for {target}: {source}")]
    HealthCheck {
        /// Underlying `DuckDB` error.
        #[source]
        source: Box<DuckdbError>,
        /// Source target.
        target: Box<DuckDBPoolTarget>,
    },
}

/// Errors that can occur when working with `DuckDB` tile sources.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum DuckDBError {
    /// Cannot build the shared `DuckDB` pool.
    #[error("Unable to build DuckDB pool {1}: {0}")]
    DuckDBPoolBuildError(#[source] BuildError, String),

    /// Cannot get a ready connection from the shared `DuckDB` pool.
    #[error("Unable to get a DuckDB connection from pool {1}: {0}")]
    DuckDBPoolConnError(#[source] PoolError<DuckDBPoolManagerError>, String),

    /// The blocking task used by `generate_tile()` failed.
    #[error("DuckDB blocking task failed while {1}: {0}")]
    DuckDBTaskJoinError(#[source] JoinError, &'static str),

    /// Query preparation failed while serving a tile.
    #[error(
        "Error preparing DuckDB query for tile source '{source_id}' ({signature}): {query} {source}"
    )]
    PrepareQueryError {
        /// The underlying `DuckDB` error.
        #[source]
        source: Box<DuckdbError>,
        /// The id of the tile source the query was prepared for.
        source_id: String,
        /// The source's query signature (parameter types).
        signature: String,
        /// The SQL query that failed to prepare.
        query: String,
    },

    /// Query execution failed while serving a tile.
    #[error(r"Unable to get tile {2:#} from DuckDB source {1}: {0}")]
    GetTileError(#[source] Box<DuckdbError>, String, TileCoord),

    /// Query execution unexpectedly received URL query parameters.
    #[error(r"Unable to get tile {2:#} with query params from DuckDB source {1}: {0}")]
    GetTileWithQueryError(
        #[source] Box<DuckdbError>,
        String,
        TileCoord,
        Option<UrlQuery>,
    ),
}

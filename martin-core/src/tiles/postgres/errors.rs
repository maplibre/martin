//! Error types for `PostgreSQL` operations.

use std::io;
use std::path::PathBuf;

use deadpool_postgres::tokio_postgres::Error as TokioPostgresError;
use deadpool_postgres::tokio_postgres::config::SslMode;
use deadpool_postgres::{BuildError, PoolError};
use martin_tile_utils::TileCoord;
use semver::Version;

use crate::tiles::UrlQuery;
use crate::tiles::postgres::RedactedConnectionString;
use crate::tiles::postgres::utils::query_to_json;

/// Result type for `PostgreSQL` operations.
pub type PostgresResult<T> = Result<T, PostgresError>;

/// Errors that can occur when working with `PostgreSQL` databases.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum PostgresError {
    /// Cannot load platform root certificates.
    #[error("Cannot load platform root certificates: {0:?}")]
    CannotLoadRoots(Vec<rustls_native_certs::Error>),

    /// Cannot open SSL certificate file.
    #[error("Cannot open certificate file {1}: {0}")]
    CannotOpenCert(#[source] io::Error, PathBuf),

    /// Cannot parse SSL certificate file.
    #[error("Cannot parse certificate file {1}: {0}")]
    CannotParseCert(#[source] io::Error, PathBuf),

    /// Invalid PEM RSA private key file.
    #[error("Unable to parse PEM RSA key file {0}")]
    InvalidPrivateKey(PathBuf),

    /// Cannot use client certificate pair.
    #[error("Unable to use client certificate pair {cert} / {key}: {source}")]
    CannotUseClientKey {
        /// The underlying `rustls` error.
        #[source]
        source: rustls::Error,
        /// Path to the client certificate file.
        cert: PathBuf,
        /// Path to the client private key file.
        key: PathBuf,
    },

    /// Wrapper for rustls errors.
    #[error(transparent)]
    RustlsError(#[from] rustls::Error),

    /// Unknown SSL mode specified.
    #[error("Unknown SSL mode: {0:?}")]
    UnknownSslMode(SslMode),

    /// `PostgreSQL` database error.
    #[error("Postgres error while {1}: {0}")]
    PostgresError(#[source] TokioPostgresError, &'static str),

    /// Cannot build `PostgreSQL` connection pool.
    #[error("Unable to build a Postgres connection pool {1}: {0}")]
    PostgresPoolBuildError(#[source] BuildError, String),

    /// Cannot get connection from `PostgreSQL` pool.
    #[error("Unable to get a Postgres connection from the pool {1}: {0}")]
    PostgresPoolConnError(#[source] PoolError, String),

    /// Invalid `PostgreSQL` connection string.
    #[error("Unable to parse connection string {1}: {0}")]
    BadConnectionString(#[source] TokioPostgresError, RedactedConnectionString),

    /// Cannot parse `PostGIS` version.
    #[error("Unable to parse PostGIS version {1}: {0}")]
    BadPostgisVersion(#[source] semver::Error, String),

    /// Cannot parse `PostgreSQL` version.
    #[error("Unable to parse PostgreSQL version {1}: {0}")]
    BadPostgresVersion(#[source] semver::Error, String),

    /// `PostGIS` version too old.
    #[error("PostGIS version {current} is too old, minimum required is {minimum}")]
    PostgisTooOld {
        /// The detected `PostGIS` version.
        current: Version,
        /// The minimum required `PostGIS` version.
        minimum: Version,
    },

    /// `PostgreSQL` version too old.
    #[error("PostgreSQL version {current} is too old, minimum required is {minimum}")]
    PostgresqlTooOld {
        /// The detected `PostgreSQL` version.
        current: Version,
        /// The minimum required `PostgreSQL` version.
        minimum: Version,
    },

    /// Query preparation error.
    #[error("Error preparing a query for the tile '{source_id}' ({signature}): {query} {source}")]
    PrepareQueryError {
        /// The underlying `PostgreSQL` error.
        #[source]
        source: TokioPostgresError,
        /// The id of the tile source the query was prepared for.
        source_id: String,
        /// The source's query signature (parameter types).
        signature: String,
        /// The SQL query that failed to prepare.
        query: String,
    },

    /// Tile retrieval error.
    #[error(r"Unable to get tile {2:#} from {1}: {0}")]
    GetTileError(#[source] TokioPostgresError, String, TileCoord),

    /// Tile retrieval error with query parameters.
    #[error(r"Unable to get tile {2:#} with {json_query:?} params from {1}: {0}", json_query=query_to_json(.3.as_ref()))]
    GetTileWithQueryError(
        #[source] TokioPostgresError,
        String,
        TileCoord,
        Option<UrlQuery>,
    ),
}

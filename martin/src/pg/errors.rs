//! Error types for PostgreSQL operations.

use std::io;
use std::path::PathBuf;

use deadpool_postgres::tokio_postgres::Error as TokioPgError;
use deadpool_postgres::{BuildError, PoolError};
use martin_tile_utils::TileCoord;
use semver::Version;

use super::utils::query_to_json;
use crate::source::UrlQuery;

/// Result type for PostgreSQL operations.
pub type PgResult<T> = Result<T, PgError>;

/// Errors that can occur when working with PostgreSQL databases.
#[derive(thiserror::Error, Debug)]
pub enum PgError {
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
    #[error("Unable to use client certificate pair {1} / {2}: {0}")]
    CannotUseClientKey(#[source] rustls::Error, PathBuf, PathBuf),

    /// Wrapper for rustls errors.
    #[error(transparent)]
    RustlsError(#[from] rustls::Error),

    /// Unknown SSL mode specified.
    #[error("Unknown SSL mode: {0:?}")]
    UnknownSslMode(deadpool_postgres::tokio_postgres::config::SslMode),

    /// PostgreSQL database error.
    #[error("Postgres error while {1}: {0}")]
    PostgresError(#[source] TokioPgError, &'static str),

    /// Cannot build PostgreSQL connection pool.
    #[error("Unable to build a Postgres connection pool {1}: {0}")]
    PostgresPoolBuildError(#[source] BuildError, String),

    /// Cannot get connection from PostgreSQL pool.
    #[error("Unable to get a Postgres connection from the pool {1}: {0}")]
    PostgresPoolConnError(#[source] PoolError, String),

    /// Invalid PostgreSQL connection string.
    #[error("Unable to parse connection string {1}: {0}")]
    BadConnectionString(#[source] TokioPgError, String),

    /// Cannot parse PostGIS version.
    #[error("Unable to parse PostGIS version {1}: {0}")]
    BadPostgisVersion(#[source] semver::Error, String),

    /// Cannot parse PostgreSQL version.
    #[error("Unable to parse PostgreSQL version {1}: {0}")]
    BadPostgresVersion(#[source] semver::Error, String),

    /// PostGIS version too old.
    #[error("PostGIS version {0} is too old, minimum required is {1}")]
    PostgisTooOld(Version, Version),

    /// PostgreSQL version too old.
    #[error("PostgreSQL version {0} is too old, minimum required is {1}")]
    PostgresqlTooOld(Version, Version),

    /// Invalid table extent configuration.
    #[error("Invalid extent setting in source {0} for table {1}: extent=0")]
    InvalidTableExtent(String, String),

    /// Query preparation error.
    #[error("Error preparing a query for the tile '{1}' ({2}): {3} {0}")]
    PrepareQueryError(#[source] TokioPgError, String, String, String),

    /// Tile retrieval error.
    #[error(r"Unable to get tile {2:#} from {1}: {0}")]
    GetTileError(#[source] TokioPgError, String, TileCoord),

    /// Tile retrieval error with query parameters.
    #[error(r"Unable to get tile {2:#} with {json_query:?} params from {1}: {0}", json_query=query_to_json(.3.as_ref()))]
    GetTileWithQueryError(#[source] TokioPgError, String, TileCoord, Option<UrlQuery>),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    ConfigError(&'static str),
}

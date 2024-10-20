use std::io;
use std::path::PathBuf;

use deadpool_postgres::tokio_postgres::Error as TokioPgError;
use deadpool_postgres::{BuildError, PoolError};
use martin_tile_utils::TileCoord;
use semver::Version;

use crate::pg::utils::query_to_json;
use crate::source::UrlQuery;

pub type PgResult<T> = Result<T, PgError>;

#[derive(thiserror::Error, Debug)]
pub enum PgError {
    #[error("Cannot load platform root certificates: {0:?}")]
    CannotLoadRoots(Vec<rustls_native_certs::Error>),

    #[error("Cannot open certificate file {}: {0}", .1.display())]
    CannotOpenCert(#[source] io::Error, PathBuf),

    #[error("Cannot parse certificate file {}: {0}", .1.display())]
    CannotParseCert(#[source] io::Error, PathBuf),

    #[error("Unable to parse PEM RSA key file {}", .0.display())]
    InvalidPrivateKey(PathBuf),

    #[error("Unable to use client certificate pair {} / {}: {0}", .1.display(), .2.display())]
    CannotUseClientKey(#[source] rustls::Error, PathBuf, PathBuf),

    #[error(transparent)]
    RustlsError(#[from] rustls::Error),

    #[error("Unknown SSL mode: {0:?}")]
    UnknownSslMode(deadpool_postgres::tokio_postgres::config::SslMode),

    #[error("Postgres error while {1}: {0}")]
    PostgresError(#[source] TokioPgError, &'static str),

    #[error("Unable to build a Postgres connection pool {1}: {0}")]
    PostgresPoolBuildError(#[source] BuildError, String),

    #[error("Unable to get a Postgres connection from the pool {1}: {0}")]
    PostgresPoolConnError(#[source] PoolError, String),

    #[error("Unable to parse connection string {1}: {0}")]
    BadConnectionString(#[source] TokioPgError, String),

    #[error("Unable to parse PostGIS version {1}: {0}")]
    BadPostgisVersion(#[source] semver::Error, String),

    #[error("Unable to parse PostgreSQL version {1}: {0}")]
    BadPostgresVersion(#[source] semver::Error, String),

    #[error("PostGIS version {0} is too old, minimum required is {1}")]
    PostgisTooOld(Version, Version),

    #[error("PostgreSQL version {0} is too old, minimum required is {1}")]
    PostgresqlTooOld(Version, Version),

    #[error("Invalid extent setting in source {0} for table {1}: extent=0")]
    InvalidTableExtent(String, String),

    #[error("Error preparing a query for the tile '{1}' ({2}): {3} {0}")]
    PrepareQueryError(#[source] TokioPgError, String, String, String),

    #[error(r#"Unable to get tile {2:#} from {1}: {0}"#)]
    GetTileError(#[source] TokioPgError, String, TileCoord),

    #[error(r#"Unable to get tile {2:#} with {:?} params from {1}: {0}"#, query_to_json(.3.as_ref()))]
    GetTileWithQueryError(#[source] TokioPgError, String, TileCoord, Option<UrlQuery>),
}

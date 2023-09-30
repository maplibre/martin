use std::io;
use std::path::PathBuf;

use deadpool_postgres::tokio_postgres::Error as TokioPgError;
use deadpool_postgres::{BuildError, PoolError};
use semver::Version;

use crate::pg::utils::query_to_json;
use crate::source::UrlQuery;
use crate::Xyz;

pub type Result<T> = std::result::Result<T, PgError>;

#[derive(thiserror::Error, Debug)]
pub enum PgError {
    #[error("Cannot load platform root certificates: {0}")]
    CantLoadRoots(#[source] io::Error),

    #[error("Cannot open certificate file {}: {0}", .1.display())]
    CantOpenCert(#[source] io::Error, PathBuf),

    #[error("Cannot parse certificate file {}: {0}", .1.display())]
    CantParseCert(#[source] io::Error, PathBuf),

    #[error("Unable to parse PEM RSA key file {}", .0.display())]
    InvalidPrivateKey(PathBuf),

    #[error("Unable to use client certificate pair {} / {}: {0}", .1.display(), .2.display())]
    CannotUseClientKey(#[source] rustls::Error, PathBuf, PathBuf),

    #[error("Rustls Error: {0:?}")]
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

    #[error("PostGIS version {0} is too old, minimum required is {1}")]
    PostgisTooOld(Version, Version),

    #[error("Invalid extent setting in source {0} for table {1}: extent=0")]
    InvalidTableExtent(String, String),

    #[error("Error preparing a query for the tile '{1}' ({2}): {3} {0}")]
    PrepareQueryError(#[source] TokioPgError, String, String, String),

    #[error(r#"Unable to get tile {2:#} from {1}: {0}"#)]
    GetTileError(#[source] TokioPgError, String, Xyz),

    #[error(r#"Unable to get tile {2:#} with {:?} params from {1}: {0}"#, query_to_json(.3))]
    GetTileWithQueryError(#[source] TokioPgError, String, Xyz, UrlQuery),
}

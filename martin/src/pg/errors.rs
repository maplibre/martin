use deadpool_postgres::tokio_postgres::Error as TokioPgError;
use deadpool_postgres::{BuildError, PoolError};
use semver::Version;

use crate::pg::utils::query_to_json;
use crate::source::UrlQuery;
use crate::Xyz;

pub type Result<T> = std::result::Result<T, PgError>;

#[derive(thiserror::Error, Debug)]
pub enum PgError {
    #[cfg(feature = "ssl")]
    #[error("Can't build TLS connection: {0}")]
    BuildSslConnectorError(#[from] openssl::error::ErrorStack),

    #[cfg(feature = "ssl")]
    #[error("Can't set trusted root certificate {}: {0}", .1.display())]
    BadTrustedRootCertError(#[source] openssl::error::ErrorStack, std::path::PathBuf),

    #[cfg(feature = "ssl")]
    #[error("Can't set client certificate {}: {0}", .1.display())]
    BadClientCertError(#[source] openssl::error::ErrorStack, std::path::PathBuf),

    #[cfg(feature = "ssl")]
    #[error("Can't set client certificate key {}: {0}", .1.display())]
    BadClientKeyError(#[source] openssl::error::ErrorStack, std::path::PathBuf),

    #[cfg(feature = "ssl")]
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

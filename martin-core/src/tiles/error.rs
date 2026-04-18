#[cfg(feature = "unstable-cog")]
use super::cog::CogError;
#[cfg(feature = "mbtiles")]
use super::mbtiles::MbtilesError;
#[cfg(feature = "pmtiles")]
use super::pmtiles::PmtilesError;
#[cfg(feature = "postgres")]
use super::postgres::PostgresError;

/// Errors that can occur during tile processing operations.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum MartinCoreError {
    /// Errors that can occur during [`mbtiles`](crate::tiles::cog) processing operations.
    #[cfg(feature = "mbtiles")]
    #[error(transparent)]
    MbtilesError(#[from] MbtilesError),

    /// Errors that can occur during [`postgres`](crate::tiles::cog) processing operations.
    #[cfg(feature = "postgres")]
    #[error(transparent)]
    PostgresError(#[from] PostgresError),

    /// Errors that can occur during [`pmtiles`](crate::tiles::cog) processing operations.
    #[cfg(feature = "pmtiles")]
    #[error(transparent)]
    PmtilesError(#[from] PmtilesError),

    /// Errors that can occur during [`cog`](crate::tiles::cog) processing operations.
    #[cfg(feature = "unstable-cog")]
    #[error(transparent)]
    CogError(#[from] CogError),

    /// Errors occurring from other sources, not implemented by `martin-core`.
    #[error(transparent)]
    OtherError(#[from] Box<dyn std::error::Error + Send + Sync>),
}

/// A convenience [`Result`] for tiles coming from `martin-core`.
pub type MartinCoreResult<T> = Result<T, MartinCoreError>;

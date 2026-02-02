/// Errors that can occur during tile processing operations.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum MartinCoreError {
    /// Errors that can occur during [`mbtiles`](crate::tiles::cog) processing operations.
    #[cfg(feature = "mbtiles")]
    #[error(transparent)]
    MbtilesError(#[from] super::mbtiles::MbtilesError),

    /// Errors that can occur during [`postgres`](crate::tiles::cog) processing operations.
    #[cfg(feature = "postgres")]
    #[error(transparent)]
    PostgresError(#[from] super::postgres::PostgresError),

    /// Errors that can occur during [`pmtiles`](crate::tiles::cog) processing operations.
    #[cfg(feature = "pmtiles")]
    #[error(transparent)]
    PmtilesError(#[from] super::pmtiles::PmtilesError),

    /// Errors that can occur during [`cog`](crate::tiles::cog) processing operations.
    #[cfg(feature = "unstable-cog")]
    #[error(transparent)]
    CogError(#[from] super::cog::CogError),

    /// Errors that can occur during [`geojson`](crate::tiles::geojson) processing operations.
    #[cfg(feature = "geojson")]
    #[error(transparent)]
    GeoJsonError(#[from] super::geojson::GeoJsonError),

    /// Errors occurring from other sources, not implemented by `martin-core`.
    #[error(transparent)]
    OtherError(#[from] Box<dyn std::error::Error>),
}

/// A convenience [`Result`] for tiles coming from `martin-core`.
pub type MartinCoreResult<T> = Result<T, MartinCoreError>;

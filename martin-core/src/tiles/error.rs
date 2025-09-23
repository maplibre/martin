/// Temporary error type for integration purposes.
pub type BoxedMartinCoreError = Box<dyn std::error::Error>;

/// Errors that can occur during mbtiles processing operations.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum MartinCoreError {
    /// Errors that can occur during mbtiles processing operations.
    #[cfg(feature = "mbtiles")]
    #[error(transparent)]
    MbtilesError(#[from] super::mbtiles::MbtilesError),

    /// Errors occurring from other sources, not implemented by `martin-core`.
    #[error(transparent)]
    OtherError(#[from] Box<dyn std::error::Error>),
}

/// A convenience [`Result`] for tiles coming from `martin-core`.
pub type MartinCoreResult<T> = Result<T, BoxedMartinCoreError>;

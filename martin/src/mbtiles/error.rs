//! Error types for MBTiles operations.

#[derive(thiserror::Error, Debug)]
pub enum MbtilesError {
    /// Failed to acquire database connection to MBTiles file.
    #[error(r"Unable to acquire connection to file: {0}")]
    AcquireConnError(String),

    /// Wrapper for underlying mbtiles library errors.
    #[error(transparent)]
    MbtilesLibraryError(#[from] mbtiles::MbtError),
}

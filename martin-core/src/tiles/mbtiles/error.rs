//! Error types for `MBTiles` operations.

use std::path::PathBuf;

/// Errors that can occur during mbtiles processing operations.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum MbtilesError {
    /// Failed to acquire database connection to `MBTiles` file.
    #[error(r"Unable to acquire connection to file: {0}")]
    AcquireConnError(String),

    /// Wrapper for underlying mbtiles library errors.
    #[error(transparent)]
    MbtilesLibraryError(#[from] mbtiles::MbtError),

    /// IO error.
    #[error("IO error {0}: {1}")]
    IoError(#[source] std::io::Error, PathBuf),

    /// Unable to parse metadata in file.
    #[error(r"Unable to parse metadata in file {1}: {0}")]
    InvalidMetadata(String, PathBuf),
}

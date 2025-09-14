//! Error types for PMTiles operations.

use pmtiles::PmtError;
use url::Url;

/// Errors that can occur when working with PMTiles files.
#[derive(thiserror::Error, Debug)]
pub enum PmtilesError {
    /// Error processing S3 source URI.
    #[error(r"Error occurred in processing S3 source uri: {0}")]
    S3SourceError(String),

    /// Wrapper for underlying PMTiles library errors.
    #[error(transparent)]
    PmtError(#[from] PmtError),

    /// PMTiles error with additional context.
    #[error(r"PMTiles error {0:?} processing {1}")]
    PmtErrorWithCtx(PmtError, String),

    /// Invalid or unparseable metadata in PMTiles file.
    #[error(r"Unable to parse metadata in file {1}: {0}")]
    InvalidUrlMetadata(String, Url),
}

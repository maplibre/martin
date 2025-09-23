//! Error types for `PMTiles` operations.

use std::path::PathBuf;

use pmtiles::PmtError;
use url::Url;

/// Errors that can occur when working with `PMTiles` files.
#[derive(thiserror::Error, Debug)]
pub enum PmtilesError {
    /// Error processing S3 source URI.
    #[error(r"Failed to parse bucket name of S3 source uri {0}")]
    S3BucketNameNotString(Url),

    /// Wrapper for underlying `PMTiles` library errors.
    #[error(transparent)]
    PmtError(#[from] PmtError),

    /// `PMTiles` error with additional context.
    #[error(r"PMTiles error {0:?} processing {1}")]
    PmtErrorWithCtx(#[source] PmtError, String),

    /// Invalid or unparseable metadata in `PMTiles` file.
    #[error(r"Unable to parse metadata in file {1}: {0}")]
    InvalidUrlMetadata(String, Url),

    /// Invalid or unparseable metadata in the `PMTiles` source.
    #[error(r"Unable to parse metadata in file {1}: {0}")]
    InvalidMetadata(String, PathBuf),

    /// IO error occurred while processing `PMTiles` file.
    #[error("IO error {0}: {1}")]
    IoError(#[source] std::io::Error, PathBuf),
}

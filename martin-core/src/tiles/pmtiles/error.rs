//! Error types for `PMTiles` operations.

use pmtiles::PmtError;

/// Errors that can occur when working with `PMTiles` files.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum PmtilesError {
    /// Wrapper for underlying `PMTiles` library errors.
    #[error(transparent)]
    PmtError(#[from] PmtError),

    /// `PMTiles` error with additional context.
    #[error(r"PMTiles error {0:?} processing {1}")]
    PmtErrorWithCtx(#[source] PmtError, String),

    /// Invalid or unparseable metadata in the `PMTiles` source.
    #[error(r"Unable to parse metadata in file {1}: {0}")]
    InvalidMetadata(String, object_store::path::Path),
    
    /// Unknown tile type encountered while processing `PMTiles` file.
    #[error("Unknown tile type for source {0} ({1} at path {2})")]
    UnknownTileType(String, String, String),
}

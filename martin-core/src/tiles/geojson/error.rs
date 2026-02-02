//! Error types for `GeoJSON` operations.

use std::path::PathBuf;

/// Errors that can occur when working with COG files.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum GeoJsonError {
    /// IO error.
    #[error("IO error {0}: {1}")]
    IoError(#[source] std::io::Error, PathBuf),
}
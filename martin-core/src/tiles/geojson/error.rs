//! Error types for `GeoJSON` operations.

use std::path::PathBuf;

/// Errors that can occur when working with GeoJSON files.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum GeoJsonError {
    /// IO error
    #[error("IO error {0}: {1}")]
    IoError(#[source] std::io::Error, PathBuf),

    /// GeoJSON parsing error
    #[error("GeoJSON parsing error: {0}")]
    GeoJsonError(#[source] geojson::errors::Error),

    /// Geozero processing error
    #[error("Geozero processing error: {0}")]
    GeozeroError(#[source] geozero::error::GeozeroError),
}

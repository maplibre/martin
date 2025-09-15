//! Error types for `GeoJSON` operations.

use std::path::PathBuf;

/// Errors that can occur during `GeoJSON` processing operations.
#[derive(thiserror::Error, Debug)]
pub enum GeoJsonError {
    /// IO error.
    #[error("IO error {0}: {1}")]
    IoError(std::io::Error, PathBuf),

    /// File is not valid `GeoJSON` format.
    #[error("File {1} is not a valid GeoJSON: {0}")]
    NotValidGeoJson(serde_json::Error, PathBuf),

    /// `GeoJSON` file contains no geometry that can be served as tiles.
    #[error("GeoJSON File {0} has no geometry which could be served as tiles")]
    NoGeometry(PathBuf),
}

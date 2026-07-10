//! Error types for `GeoJSON` operations.

use std::path::PathBuf;

/// Errors that can occur when working with `GeoJSON` files.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum GeoJsonError {
    /// IO error
    #[error("IO error {0}: {1}")]
    IoError(#[source] std::io::Error, PathBuf),

    /// `GeoJSON` parsing error
    #[error("GeoJSON parsing error: {0}")]
    GeoJsonError(#[source] Box<geojson::errors::Error>),

    /// MVT encoding error
    #[error("MVT encoding error: {0}")]
    MvtError(#[source] fast_mvt::MvtError),

    /// A feature property cannot be represented as an MVT value
    #[error("GeoJSON property {0} cannot be represented as an MVT value")]
    UnsupportedProperty(String),

    /// More features than can be spatially indexed
    #[error("GeoJSON has too many features to index: {0} exceeds u32::MAX")]
    TooManyFeatures(usize),
}

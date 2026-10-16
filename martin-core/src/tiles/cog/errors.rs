//! Error types for `Cloud Optimized GeoTIFF` operations.

use std::sync::Arc;

/// Errors that can occur when working with COG files.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum CogError {
    /// Cannot open, read, or serve a COG.
    #[error(transparent)]
    Tiles(#[from] cog_tiles::CogError),

    /// The COG produces tiles in a format Martin cannot serve.
    #[error("{location} produces {format:?} tiles, which Martin cannot serve")]
    UnsupportedFormat {
        /// Display form of the source.
        location: String,
        /// The tile format reported by the COG reader.
        format: cog_tiles::TileFormat,
    },

    /// Cannot list a mosaic's files to find one to describe the mosaic with.
    #[error("Unable to list mosaic files under {location}: {source}")]
    MosaicListing {
        /// Display form of the mosaic location.
        location: String,
        /// The underlying object store error.
        #[source]
        source: object_store::Error,
    },

    /// A mosaic's file name template could not be parsed.
    #[error("Invalid mosaic file name template {template:?}: {reason}")]
    InvalidTemplate {
        /// The template as given.
        template: String,
        /// What is wrong with it.
        reason: String,
    },

    /// A file in a mosaic does not match its name or the rest of the mosaic.
    #[error("Mosaic file {key} is inconsistent with the mosaic: {reason}")]
    MosaicMismatch {
        /// The object key or path of the file, relative to the mosaic's location.
        key: String,
        /// What does not match.
        reason: String,
    },

    /// An error opening a mosaic file, shared by every request that waited for that open.
    #[error(transparent)]
    Shared(Arc<Self>),

    /// No file under a mosaic's location matches its key template.
    #[error("No file matching {template} was found under {location}")]
    MosaicEmpty {
        /// Display form of the mosaic location.
        location: String,
        /// The key template the files must match.
        template: String,
    },
}

impl crate::Classify for CogError {
    fn kind(&self) -> crate::ErrorKind {
        crate::ErrorKind::Internal
    }
}

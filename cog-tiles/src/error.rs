use std::ops::Range;

use async_tiff::error::AsyncTiffError;

/// A specialized [`Result`] type for `cog-tiles` operations.
pub type CogResult<T> = Result<T, CogError>;

/// Errors that can occur while opening a COG or reading its tiles.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum CogError {
    /// The backend failed to read a byte range.
    #[error("Unable to read {location}: {source}")]
    Backend {
        /// A display form of the source, e.g. a path or URL.
        location: String,
        /// The underlying error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// A byte range outside the source was requested.
    #[error("Invalid byte range {requested:?} for {location} of size {size}")]
    InvalidRange {
        /// A display form of the source.
        location: String,
        /// The requested byte range.
        requested: Range<u64>,
        /// The size of the source.
        size: u64,
    },

    /// The TIFF structure could not be parsed or a tile could not be decoded.
    #[error("Unable to decode TIFF {location}: {source}")]
    Tiff {
        /// A display form of the source.
        location: String,
        /// The underlying error.
        #[source]
        source: AsyncTiffError,
    },

    /// The TIFF is valid, but it cannot be read as web map tiles.
    #[error("{location} cannot be read as tiles: {reason}")]
    Unsupported {
        /// A display form of the source.
        location: String,
        /// Why the file cannot be read as tiles.
        reason: String,
    },

    /// A tile coordinate is outside the valid range for its zoom level.
    #[error("Invalid tile coordinate {z}/{x}/{y}")]
    InvalidTileCoord {
        /// Zoom level.
        z: u8,
        /// Column.
        x: u32,
        /// Row.
        y: u32,
    },
}

impl CogError {
    pub(crate) fn unsupported(location: &str, reason: impl Into<String>) -> Self {
        Self::Unsupported {
            location: location.to_owned(),
            reason: reason.into(),
        }
    }

    pub(crate) fn tiff(location: &str, source: AsyncTiffError) -> Self {
        Self::Tiff {
            location: location.to_owned(),
            source,
        }
    }
}

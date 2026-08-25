//! Error types for sprite processing operations.

use std::path::PathBuf;

use spreet::SpreetError;
use spreet::resvg::usvg::Error as ResvgError;

/// Errors that can occur during sprite processing.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum SpriteError {
    /// Sprite source ID not found.
    #[error("Sprite {0} not found")]
    SpriteNotFound(String),

    /// Too many distinct sprite source IDs requested at once.
    #[error("Requested {requested} sprite ids, but at most {max} are allowed per request")]
    TooManySpriteIds {
        /// Number of distinct ids in the request.
        requested: usize,
        /// Maximum number of distinct ids allowed per request.
        max: usize,
    },

    /// I/O error accessing sprite file or directory.
    #[error("IO error {0}: {1}")]
    IoError(#[source] std::io::Error, PathBuf),

    /// Path is not a valid file.
    #[error("Sprite path is not a file: {0}")]
    InvalidFilePath(PathBuf),

    /// Sprite source has invalid file path.
    #[error("Sprite {0} uses bad file {1}")]
    InvalidSpriteFilePath(String, PathBuf),

    /// No SVG files found in directory.
    #[error("No sprite SVG files found in {0} to generate spritesheets from")]
    NoSpriteFilesFound(PathBuf),

    /// Failed to read sprite file.
    #[error("Sprite {0} could not be loaded")]
    UnableToReadSprite(PathBuf),

    /// Sprite processing error.
    #[error("{0} in file {1}")]
    SpriteProcessingError(#[source] SpreetError, PathBuf),

    /// SVG parsing error.
    #[error("{0} in file {1}")]
    SpriteParsingError(#[source] ResvgError, PathBuf),

    /// Failed to generate spritesheet.
    #[error("Unable to generate spritesheet")]
    UnableToGenerateSpritesheet,

    /// Failed to create sprite from SVG file.
    #[error("Unable to create a sprite from file {0}")]
    SpriteInstError(PathBuf),
}

impl crate::Classify for SpriteError {
    fn kind(&self) -> crate::ErrorKind {
        use crate::ErrorKind::{Internal, InvalidInput, NotFound};
        match self {
            Self::SpriteNotFound(_) => NotFound,
            Self::TooManySpriteIds { .. } => InvalidInput,
            Self::IoError(..)
            | Self::InvalidFilePath(_)
            | Self::InvalidSpriteFilePath(..)
            | Self::NoSpriteFilesFound(_)
            | Self::UnableToReadSprite(_)
            | Self::SpriteProcessingError(..)
            | Self::SpriteParsingError(..)
            | Self::UnableToGenerateSpritesheet
            | Self::SpriteInstError(_) => Internal,
        }
    }
}

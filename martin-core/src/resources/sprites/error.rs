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
    #[error("No sprite files found in {0}")]
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

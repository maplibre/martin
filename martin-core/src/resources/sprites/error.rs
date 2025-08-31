use std::path::PathBuf;

use spreet::SpreetError;
use spreet::resvg::usvg::Error as ResvgError;

#[derive(thiserror::Error, Debug)]
pub enum SpriteError {
    #[error("Sprite {0} not found")]
    SpriteNotFound(String),

    #[error("IO error {0}: {1}")]
    IoError(std::io::Error, PathBuf),

    #[error("Sprite path is not a file: {0}")]
    InvalidFilePath(PathBuf),

    #[error("Sprite {0} uses bad file {1}")]
    InvalidSpriteFilePath(String, PathBuf),

    #[error("No sprite files found in {0}")]
    NoSpriteFilesFound(PathBuf),

    #[error("Sprite {0} could not be loaded")]
    UnableToReadSprite(PathBuf),

    #[error("{0} in file {1}")]
    SpriteProcessingError(SpreetError, PathBuf),

    #[error("{0} in file {1}")]
    SpriteParsingError(ResvgError, PathBuf),

    #[error("Unable to generate spritesheet")]
    UnableToGenerateSpritesheet,

    #[error("Unable to create a sprite from file {0}")]
    SpriteInstError(PathBuf),
}

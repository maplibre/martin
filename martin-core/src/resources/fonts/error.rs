//! Error types for font processing and serving operations.

use std::path::PathBuf;

use pbf_font_tools::PbfFontError;

use super::CP_RANGE_SIZE;

/// Errors that can occur during font processing operations.
#[derive(thiserror::Error, Debug)]
pub enum FontError {
    /// The requested font ID was not found in the font catalog.
    #[error("Font {0} not found")]
    FontNotFound(String),

    /// The font range start value is greater than the end value.
    #[error("Font range start ({0}) must be <= end ({1})")]
    InvalidFontRangeStartEnd(u32, u32),

    /// The font range start is not aligned to a 256-character boundary.
    #[error("Font range start ({0}) must be multiple of {CP_RANGE_SIZE} (e.g. 0, 256, 512, ...)")]
    InvalidFontRangeStart(u32),

    /// The font range end is not aligned to a 256-character boundary.
    #[error(
        "Font range end ({0}) must be multiple of {CP_RANGE_SIZE} - 1 (e.g. 255, 511, 767, ...)"
    )]
    InvalidFontRangeEnd(u32),

    /// The font range span is not exactly 256 characters.
    #[error(
        "Given font range {0}-{1} is invalid. It must be {CP_RANGE_SIZE} characters long (e.g. 0-255, 256-511, ...)"
    )]
    InvalidFontRange(u32, u32),

    /// An error occurred in the `FreeType` font rendering library.
    #[error(transparent)]
    FreeType(#[from] pbf_font_tools::freetype::Error),

    /// An I/O error occurred while accessing a font file or directory.
    #[error("IO error accessing {1}: {0}")]
    IoError(std::io::Error, PathBuf),

    /// The specified path is not a valid font file (supports .ttf, .otf, .ttc).
    #[error("Invalid font file {0}")]
    InvalidFontFilePath(PathBuf),

    /// No font files were discovered in the specified directory.
    #[error("No font files found in {0}")]
    NoFontFilesFound(PathBuf),

    /// A font file is missing required family name metadata.
    #[error("Font {0} is missing a family name")]
    MissingFamilyName(PathBuf),

    /// An error occurred during Protocol Buffer font processing.
    #[error(transparent)]
    PbfFontError(#[from] PbfFontError),

    /// Failed to serialize font data to Protocol Buffer format.
    #[error(transparent)]
    ErrorSerializingProtobuf(#[from] pbf_font_tools::prost::DecodeError),
}

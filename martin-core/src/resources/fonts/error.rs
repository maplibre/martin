use std::path::PathBuf;

use pbf_font_tools::PbfFontError;

use super::CP_RANGE_SIZE;

#[derive(thiserror::Error, Debug)]
pub enum FontError {
    #[error("Font {0} not found")]
    FontNotFound(String),

    #[error("Font range start ({0}) must be <= end ({1})")]
    InvalidFontRangeStartEnd(u32, u32),

    #[error("Font range start ({0}) must be multiple of {CP_RANGE_SIZE} (e.g. 0, 256, 512, ...)")]
    InvalidFontRangeStart(u32),

    #[error(
        "Font range end ({0}) must be multiple of {CP_RANGE_SIZE} - 1 (e.g. 255, 511, 767, ...)"
    )]
    InvalidFontRangeEnd(u32),

    #[error(
        "Given font range {0}-{1} is invalid. It must be {CP_RANGE_SIZE} characters long (e.g. 0-255, 256-511, ...)"
    )]
    InvalidFontRange(u32, u32),

    #[error(transparent)]
    FreeType(#[from] pbf_font_tools::freetype::Error),

    #[error("IO error accessing {1}: {0}")]
    IoError(std::io::Error, PathBuf),

    #[error("Invalid font file {0}")]
    InvalidFontFilePath(PathBuf),

    #[error("No font files found in {0}")]
    NoFontFilesFound(PathBuf),

    #[error("Font {0} is missing a family name")]
    MissingFamilyName(PathBuf),

    #[error(transparent)]
    PbfFontError(#[from] PbfFontError),

    #[error(transparent)]
    ErrorSerializingProtobuf(#[from] pbf_font_tools::prost::DecodeError),
}

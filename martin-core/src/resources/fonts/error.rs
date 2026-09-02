//! Error types for font processing and serving operations.

use std::path::PathBuf;

use pbf_font_tools::PbfFontError;

use crate::resources::fonts::CP_RANGE_SIZE;

/// Errors that can occur during font processing operations.
#[derive(thiserror::Error, Debug)]
pub enum FontError {
    /// The requested font ID was not found in the font catalog.
    #[error("Font {0} not found")]
    FontNotFound(String),

    /// Too many distinct font IDs requested at once.
    #[error("Requested {requested} font ids, but at most {max} are allowed per request")]
    TooManyFontIds {
        /// Requested count.
        requested: usize,
        /// Allowed maximum.
        max: usize,
    },

    /// A font alias name is empty or contains a comma.
    #[error(
        "Font alias {0:?} is invalid: alias names must be non-empty and must not contain commas"
    )]
    InvalidAliasName(String),

    /// A font alias does not reference any fonts.
    #[error("Font alias {0:?} does not reference any fonts")]
    EmptyAlias(String),

    /// A font alias references more fonts than a single request may use.
    #[error("Font alias {alias:?} references {requested} fonts, but at most {max} are allowed")]
    TooManyFontsInAlias {
        /// Alias name.
        alias: String,
        /// Referenced font count.
        requested: usize,
        /// Allowed maximum.
        max: usize,
    },

    /// A font alias references another alias instead of a font.
    #[error(
        "Font alias {alias:?} references {font:?}, which is itself an alias; aliases may only reference fonts"
    )]
    AliasWithinAlias {
        /// Alias name.
        alias: String,
        /// The referenced alias.
        font: String,
    },

    /// A font alias references a font that was not discovered.
    #[error("Font alias {alias:?} references unknown font {font:?}")]
    AliasFontNotFound {
        /// Alias name.
        alias: String,
        /// The referenced font.
        font: String,
    },

    /// The font range start value is greater than the end value.
    #[error("Font range start ({start}) must be <= end ({end})")]
    InvalidFontRangeStartEnd {
        /// The requested range start codepoint.
        start: u32,
        /// The requested range end codepoint.
        end: u32,
    },

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
    IoError(#[source] std::io::Error, PathBuf),

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

impl crate::Classify for FontError {
    fn kind(&self) -> crate::ErrorKind {
        use crate::ErrorKind::{Internal, InvalidInput, NotFound};
        match self {
            Self::FontNotFound(_) => NotFound,
            Self::TooManyFontIds { .. }
            | Self::InvalidFontRangeStartEnd { .. }
            | Self::InvalidFontRangeStart(_)
            | Self::InvalidFontRangeEnd(_)
            | Self::InvalidFontRange(_, _)
            | Self::InvalidAliasName(_)
            | Self::EmptyAlias(_)
            | Self::TooManyFontsInAlias { .. }
            | Self::AliasWithinAlias { .. }
            | Self::AliasFontNotFound { .. } => InvalidInput,
            Self::FreeType(_)
            | Self::IoError(..)
            | Self::InvalidFontFilePath(_)
            | Self::NoFontFilesFound(_)
            | Self::MissingFamilyName(_)
            | Self::PbfFontError(_)
            | Self::ErrorSerializingProtobuf(_) => Internal,
        }
    }
}

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

    /// A mosaic's file name template is invalid.
    #[error("Invalid mosaic file name template {template:?}: {reason}")]
    InvalidTemplate {
        /// The template as given.
        template: String,
        /// What is wrong with it.
        reason: MosaicTemplateError,
    },

    /// A mosaic's file zoom is above the highest zoom a COG can have.
    #[error("The mosaic file zoom {file_zoom} is above the highest zoom {}", cog_tiles::MAX_ZOOM)]
    FileZoomTooHigh {
        /// The configured file zoom.
        file_zoom: u8,
    },

    /// One file of a mosaic cannot be served.
    #[error("Unable to serve mosaic file {key}: {source}")]
    MosaicFile {
        /// The object key or path of the file, relative to the mosaic's location.
        key: String,
        /// Why the file cannot be served, handed to every request that waited for the same open.
        #[source]
        source: Arc<MosaicFileError>,
    },

    /// No file under a mosaic's location matches its key template.
    #[error("No file matching {template} was found under {location}")]
    MosaicEmpty {
        /// Display form of the mosaic location.
        location: String,
        /// The key template the files must match.
        template: String,
    },
}

/// Why a mosaic's file name template is invalid.
#[non_exhaustive]
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum MosaicTemplateError {
    /// A `}` without a `{` before it.
    #[error("unmatched `}}`")]
    UnmatchedClose,
    /// A `{` without a `}` after it.
    #[error("unmatched `{{`")]
    UnmatchedOpen,
    /// A placeholder other than the supported ones.
    #[error("unknown placeholder {{{0}}}, placeholders are {{quadkey}}, {{z}}, {{x}}, {{y}} and {{-y}}")]
    UnknownPlaceholder(String),
    /// Neither a quadkey nor both a column and a row.
    #[error("it must contain {{quadkey}}, or {{x}} and {{y}} (or {{-y}})")]
    MissingCoordinates,
    /// The names it renders leave the mosaic's location, e.g. through `..` or a leading `/`.
    #[error("file names must be relative paths inside the mosaic's location")]
    NotRelative,
}

/// Why one file of a mosaic cannot be served.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum MosaicFileError {
    /// Cannot open or read the file.
    #[error(transparent)]
    Tiles(#[from] cog_tiles::CogError),
    /// The file does not cover the tile its name stands for.
    #[error("it does not cover tile {tile}")]
    DoesNotCover {
        /// The tile the file's name stands for.
        tile: cog_tiles::TileCoord,
    },
    /// The file has no tiles at the mosaic's file zoom.
    #[error("its highest zoom {max_zoom} is below the file zoom {file_zoom}")]
    BelowFileZoom {
        /// The file's highest zoom.
        max_zoom: u8,
        /// The mosaic's file zoom.
        file_zoom: u8,
    },
    /// The file's tiles differ in size or format from the first file the mosaic opened.
    #[error(
        "it has {tile_size}px {format:?} tiles, but the mosaic has {expected_tile_size}px {expected_format:?} tiles"
    )]
    Layout {
        /// The file's tile size in pixels.
        tile_size: u32,
        /// The file's tile format.
        format: cog_tiles::TileFormat,
        /// The mosaic's tile size in pixels.
        expected_tile_size: u32,
        /// The mosaic's tile format.
        expected_format: cog_tiles::TileFormat,
    },
}

impl crate::Classify for CogError {
    fn kind(&self) -> crate::ErrorKind {
        crate::ErrorKind::Internal
    }
}

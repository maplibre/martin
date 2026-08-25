//! The error space of a hillshade bake.

use martin_tile_utils::Format;

use crate::tiles::neighbourhood::NeighbourhoodError;

/// Errors raised while assembling, baking, or encoding a hillshade tile.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum HillshadeError {
    /// The centre tile was fetched successfully but could not be decoded as an image.
    #[error(
        "The centre normal tile was fetched but could not be decoded as an image. \
         The upstream source served malformed data for this tile."
    )]
    CorruptCentreTile,

    /// A bake produced a buffer that did not match its own declared dimensions.
    #[error(
        "Baked hillshade is {actual} bytes but its {side}x{side} dimensions require {expected}"
    )]
    MalformedBake {
        /// Byte length produced.
        actual: usize,
        /// Byte length the declared dimensions require.
        expected: usize,
        /// Declared side length in pixels.
        side: u32,
    },

    /// A format was requested that the hillshade encoder cannot produce.
    #[error(
        "Hillshade cannot be encoded as {0}. Supported formats are png and webp, \
         both lossless. A hillshade is multiplied over the basemap, so a lossy \
         codec would show visible blotches on flat terrain."
    )]
    UnsupportedFormat(Format),

    /// Encoding the baked grayscale image failed.
    #[error("Failed to encode the baked hillshade as {format}: {source}")]
    Encoding {
        /// Target image format.
        format: Format,
        /// Underlying encoder error.
        source: image::ImageError,
    },
}

impl From<NeighbourhoodError> for HillshadeError {
    fn from(value: NeighbourhoodError) -> Self {
        match value {
            NeighbourhoodError::CorruptCentreTile => Self::CorruptCentreTile,
        }
    }
}

impl crate::Classify for HillshadeError {
    fn kind(&self) -> crate::ErrorKind {
        use crate::ErrorKind::{Internal, InvalidInput, Unavailable};
        match self {
            Self::CorruptCentreTile => Unavailable,
            Self::UnsupportedFormat(_) => InvalidInput,
            Self::MalformedBake { .. } | Self::Encoding { .. } => Internal,
        }
    }
}

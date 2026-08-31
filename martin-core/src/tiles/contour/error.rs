//! The error space of a contour build.

use crate::tiles::neighbourhood::NeighbourhoodError;

/// Errors raised while assembling, tracing, or encoding a contour tile.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum ContourError {
    /// The centre tile was fetched successfully but could not be decoded as an image.
    #[error(
        "The centre elevation tile was fetched but could not be decoded as an image. \
         The upstream source served malformed data for this tile."
    )]
    CorruptCentreTile,

    /// Marching squares failed on the elevation grid.
    #[error("Failed to trace isolines through the elevation grid: {0}")]
    Isolines(String),

    /// The MVT encoder rejected a feature.
    #[error("Failed to encode the traced contours as MVT: {0}")]
    Encoding(String),
}

impl From<NeighbourhoodError> for ContourError {
    fn from(value: NeighbourhoodError) -> Self {
        match value {
            NeighbourhoodError::CorruptCentreTile => Self::CorruptCentreTile,
        }
    }
}

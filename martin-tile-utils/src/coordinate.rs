//! Tile coordinates in the XYZ tiling scheme.

use std::fmt::{Display, Formatter};

use crate::MAX_ZOOM;

/// A single tile address in the XYZ tiling scheme.
///
/// # Examples
///
/// ```
/// # use martin_tile_utils::TileCoord;
/// let coord = TileCoord::new_unchecked(4, 2, 3);
/// assert_eq!((coord.z(), coord.x(), coord.y()), (4, 2, 3));
/// ```
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
#[must_use]
pub struct TileCoord {
    z: u8,
    x: u32,
    y: u32,
}

impl Display for TileCoord {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            write!(f, "{}/{}/{}", self.z(), self.x(), self.y())
        } else {
            write!(f, "{},{},{}", self.z(), self.x(), self.y())
        }
    }
}

impl TileCoord {
    /// Checks provided coordinates for validity
    /// before constructing [`TileCoord`] instance.
    ///
    /// Check [`Self::new_unchecked`] if you are sure that your inputs are possible.
    #[must_use]
    pub fn new_checked(z: u8, x: u32, y: u32) -> Option<Self> {
        Self::is_possible_on_zoom_level(z, x, y).then_some(Self { z, x, y })
    }

    /// Constructs [`TileCoord`] instance from arguments without checking that the tiles can exist.
    ///
    /// Check [`Self::new_checked`] if you are unsure if your inputs are possible.
    pub const fn new_unchecked(z: u8, x: u32, y: u32) -> Self {
        Self { z, x, y }
    }

    /// Checks that zoom `z` is plausibily small and `x`/`y` is possible on said zoom level
    #[must_use]
    pub const fn is_possible_on_zoom_level(z: u8, x: u32, y: u32) -> bool {
        if z > MAX_ZOOM {
            return false;
        }

        let side_len = 1_u32 << z;
        x < side_len && y < side_len
    }

    /// The zoom level of this tile
    #[must_use]
    pub const fn z(self) -> u8 {
        self.z
    }

    /// The column of this tile, counted from the left edge of the world
    #[must_use]
    pub const fn x(self) -> u32 {
        self.x
    }

    /// The row of this tile, counted from the top edge of the world
    #[must_use]
    pub const fn y(self) -> u32 {
        self.y
    }
}

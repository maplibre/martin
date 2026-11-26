//! Tile coordinates in the XYZ tiling scheme.

use std::fmt::{Debug, Display, Formatter};
use std::num::NonZeroU64;

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
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
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

    /// Packs this coordinate into one `u64`, for compact and cheap-to-hash keys.
    ///
    /// The coordinate must be on a grid at most two tiles wide or tall at zoom 0, like every [`crate::TileGrid`]:
    /// `z <= MAX_ZOOM` and `x, y < 2^(z + 1)`.
    pub const fn pack(self) -> PackedTileCoord {
        debug_assert!(self.z <= MAX_ZOOM && (self.x as u64) < 2 << self.z && (self.y as u64) < 2 << self.z);
        let bits = self.z as u32 + 1;
        let packed = (1_u64 << (2 * bits)) | ((self.x as u64) << bits) | self.y as u64;
        PackedTileCoord(NonZeroU64::new(packed).expect("the zoom marker bit is always set"))
    }
}

/// A [`TileCoord`] in one non-zero `u64`: a marker bit at `2(z + 1)`, then `x` and `y` in `z + 1` bits each.
///
/// The extra bit per axis fits grids two tiles wide or tall at zoom 0, such as `WorldCRS84Quad`.
/// The marker makes every coordinate on every zoom level a distinct value,
/// and the niche lets `Option<PackedTileCoord>` stay 8 bytes.
#[derive(Copy, Clone, Hash, PartialEq, Eq)]
#[must_use]
pub struct PackedTileCoord(NonZeroU64);

impl PackedTileCoord {
    /// The coordinate this value was packed from.
    pub const fn unpack(self) -> TileCoord {
        let packed = self.0.get();
        let bits = self.0.ilog2() / 2;
        let mask = (1_u64 << bits) - 1;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "z <= 31 and x, y < 2^(z + 1) by construction"
        )]
        TileCoord::new_unchecked(
            (bits - 1) as u8,
            ((packed >> bits) & mask) as u32,
            (packed & mask) as u32,
        )
    }
}

impl Debug for PackedTileCoord {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.unpack(), f)
    }
}

impl From<TileCoord> for PackedTileCoord {
    fn from(coord: TileCoord) -> Self {
        coord.pack()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_round_trips_every_corner_of_a_two_wide_and_two_tall_grid() {
        for z in 0..=MAX_ZOOM {
            let max = (2_u32 << z) - 1;
            for (x, y) in [(0, 0), (max, 0), (0, max), (max, max), (max / 2, max / 3)] {
                let coord = TileCoord::new_unchecked(z, x, y);
                assert_eq!(coord.pack().unpack(), coord);
            }
        }
    }

    #[test]
    fn pack_tells_zoom_levels_apart() {
        let last_of_each_zoom = (0..=MAX_ZOOM).map(|z| {
            let max = (2_u32 << z) - 1;
            TileCoord::new_unchecked(z, max, max).pack().0.get()
        });
        let first_of_each_zoom = (0..=MAX_ZOOM).map(|z| TileCoord::new_unchecked(z, 0, 0).pack().0.get());
        for (last, next_first) in last_of_each_zoom.zip(first_of_each_zoom.skip(1)) {
            assert!(last < next_first);
        }
    }

    #[test]
    fn both_zoom_0_tiles_of_a_two_wide_grid_get_their_own_value() {
        assert_ne!(
            TileCoord::new_unchecked(0, 0, 0).pack(),
            TileCoord::new_unchecked(0, 1, 0).pack()
        );
        assert_ne!(
            TileCoord::new_unchecked(0, 0, 0).pack(),
            TileCoord::new_unchecked(0, 0, 1).pack()
        );
    }

    #[test]
    fn packed_debug_shows_the_coordinate() {
        assert_eq!(
            format!("{:?}", TileCoord::new_unchecked(4, 2, 3).pack()),
            "TileCoord { z: 4, x: 2, y: 3 }"
        );
    }

    #[test]
    fn option_of_packed_fits_in_eight_bytes() {
        assert_eq!(size_of::<Option<PackedTileCoord>>(), 8);
    }
}

//! Tile addressing on the integer grid.
//!
//! A tile of side `extent` covers the closed integer square `[x·extent, x·extent + extent − 1]` on
//! each axis, so the boundary between tiles `k−1` and `k` sits at `k·extent − 0.5` (between two
//! integer coordinates). Integer vertices therefore never land exactly on a tile edge — every vertex
//! belongs to exactly one tile.

use geo_types::Coord;

/// A tile address on the integer grid. Coordinates may be negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileId {
    pub x: i32,
    pub y: i32,
}

impl TileId {
    #[must_use]
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// This tile's origin in global coordinates — `self · extent`, the global position its local
    /// `[0, 0]` corner maps to. **Add** it to a tile-local coordinate to lift it into the global
    /// frame, or **subtract** it to drop a global coordinate into this tile's local frame.
    ///
    /// Returns `None` if `self · extent` overflows `i32` (a tile far outside the representable range
    /// for this `extent`).
    ///
    /// ```
    /// # use map_tile_toolkit::TileId;
    /// # use geo_types::Coord;
    /// let origin = TileId::new(2, 3).origin(25).expect("in range");
    /// assert_eq!(origin, Coord { x: 50, y: 75 });
    /// assert_eq!(Coord { x: 4, y: 1 } + origin, Coord { x: 54, y: 76 }); // local → global
    /// assert_eq!(Coord { x: 54, y: 76 } - origin, Coord { x: 4, y: 1 }); // global → local
    /// ```
    #[must_use]
    pub fn origin(self, extent: u32) -> Option<Coord<i32>> {
        let e = i64::from(extent);
        Some(Coord {
            x: i32::try_from(i64::from(self.x) * e).ok()?,
            y: i32::try_from(i64::from(self.y) * e).ok()?,
        })
    }
}

impl From<(i32, i32)> for TileId {
    fn from((x, y): (i32, i32)) -> Self {
        Self { x, y }
    }
}

/// The tile that owns coordinate `c` for the given tile side `extent`.
pub(crate) fn tile_of(c: Coord<i32>, extent: i32) -> TileId {
    TileId::new(c.x.div_euclid(extent), c.y.div_euclid(extent))
}

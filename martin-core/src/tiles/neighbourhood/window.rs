//! Locating a requested tile inside an ancestor's centre tile.

use martin_tile_utils::TileCoord;

/// Where the requested tile sits inside the centre tile of the neighbourhood it is read from.
///
/// A source with variable-depth coverage answers some zooms only in places.
/// A tile it does not hold is read from its nearest ancestor instead, and this records which part of that ancestor to upsample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct AncestorWindow {
    /// Zoom levels between the requested tile and the ancestor, `0` for the tile itself.
    levels: u8,
    /// Column of the requested tile among the ancestor's `2^levels` columns of descendants.
    column: u32,
    /// Row of the requested tile among the ancestor's `2^levels` rows of descendants.
    row: u32,
}

impl AncestorWindow {
    /// The window of a tile read at its own zoom.
    pub const NATIVE: Self = Self {
        levels: 0,
        column: 0,
        row: 0,
    };

    /// The ancestor `levels` zooms above `tile`, and where `tile` sits inside it.
    ///
    /// # Panics
    ///
    /// Panics if `levels` exceeds the zoom of `tile`.
    pub fn above(tile: TileCoord, levels: u8) -> (TileCoord, Self) {
        assert!(levels <= tile.z(), "an ancestor cannot be above zoom 0");
        let mask = (1u32 << levels) - 1;
        let ancestor = TileCoord::new_unchecked(tile.z() - levels, tile.x() >> levels, tile.y() >> levels);
        let window = Self {
            levels,
            column: tile.x() & mask,
            row: tile.y() & mask,
        };
        (ancestor, window)
    }

    /// Whether this is the tile itself rather than part of an ancestor.
    #[must_use]
    pub const fn is_native(self) -> bool {
        self.levels == 0
    }

    /// Maps the centre of pixel `pixel` along one axis of the requested tile into the ancestor's pixel space.
    ///
    /// `pixel` may run past either edge of the tile, into the fetch apron.
    /// The result is relative to the ancestor tile's own origin, with integer `n` at the centre of the ancestor's pixel `n`.
    #[must_use]
    pub fn ancestor_x(self, tile_size: usize, pixel: f64) -> f64 {
        self.map(self.column, tile_size, pixel)
    }

    /// The vertical counterpart of [`ancestor_x`](Self::ancestor_x).
    #[must_use]
    pub fn ancestor_y(self, tile_size: usize, pixel: f64) -> f64 {
        self.map(self.row, tile_size, pixel)
    }

    /// Shared body of [`ancestor_x`](Self::ancestor_x) and [`ancestor_y`](Self::ancestor_y).
    #[expect(clippy::cast_precision_loss, reason = "a tile side is a small power of two")]
    fn map(self, offset: u32, tile_size: usize, pixel: f64) -> f64 {
        let scale = f64::from(1u32 << self.levels);
        (f64::from(offset) * tile_size as f64 + pixel + 0.5) / scale - 0.5
    }
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;

    use super::*;

    #[test]
    fn a_tile_zero_levels_above_is_itself() {
        let tile = TileCoord::new_unchecked(11, 326, 793);
        assert_eq!(AncestorWindow::above(tile, 0), (tile, AncestorWindow::NATIVE));
    }

    #[test]
    fn an_ancestor_is_the_parent_chain_and_the_window_the_remainder() {
        let (ancestor, window) = AncestorWindow::above(TileCoord::new_unchecked(13, 1307, 3174), 3);
        assert_eq!(ancestor, TileCoord::new_unchecked(10, 163, 396));
        assert_eq!(
            window,
            AncestorWindow {
                levels: 3,
                column: 3,
                row: 6
            }
        );
    }

    #[test]
    fn a_native_window_maps_every_pixel_onto_itself() {
        for pixel in [-32.0, 0.0, 17.0, 511.0, 543.0] {
            assert_relative_eq!(AncestorWindow::NATIVE.ancestor_x(512, pixel), pixel);
        }
    }

    #[test]
    fn a_child_window_maps_its_edges_onto_the_matching_ancestor_half() {
        let (_, window) = AncestorWindow::above(TileCoord::new_unchecked(11, 327, 792), 1);
        assert_relative_eq!(window.ancestor_x(512, -0.5), 256.0 - 0.5);
        assert_relative_eq!(window.ancestor_x(512, 511.5), 512.0 - 0.5);
        assert_relative_eq!(window.ancestor_y(512, -0.5), -0.5);
        assert_relative_eq!(window.ancestor_y(512, 511.5), 256.0 - 0.5);
    }
}

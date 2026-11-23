//! Planetiler's point label grid ids, ported bit for bit so both put a point in the same square.

use geo_types::Coord;

use crate::phase1::TILE_PIXELS;
use crate::plan::ThinningGrid;

/// The grid square of world coordinate `c` at `zoom`, as Planetiler's `GeoUtils.labelGridId`: the
/// square's column in the high 32 bits and its row in the low ones, without the grid size. `None` for a
/// square smaller than one unit of a tile `extent` units wide, where no two points could share it.
#[expect(clippy::cast_possible_truncation, reason = "saturates as Java's (int) cast does")]
pub(crate) fn group(grid: &ThinningGrid, zoom: u8, extent: u32, c: Coord<f64>) -> Option<u64> {
    let square = grid.size / TILE_PIXELS;
    if square < 1.0 / f64::from(extent) {
        return None;
    }
    let tiles = f64::from(1u32 << zoom);
    let column = (wrap(c.x * tiles, tiles) / square).floor() as i32;
    let row = ((c.y * tiles) / square).floor() as i32;
    Some((u64::from(column.cast_unsigned()) << 32) | u64::from(row.cast_unsigned()))
}

/// `value` modulo `max`, in `[0, max)`, as Planetiler's `GeoUtils.wrapDouble`.
fn wrap(value: f64, max: f64) -> f64 {
    let value = value % max;
    if value < 0.0 { value + max } else { value }
}

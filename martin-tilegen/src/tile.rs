use std::ops::Range;

use martin_tile_utils::TileCoord;
use pmtiles::{PYRAMID_SIZE_BY_ZOOM, TileId};

use crate::{TileGenError, TileGenResult};

/// Highest zoom whose tile ids leave the low byte of a [`SortKey`](crate::SortKey) free for the layer.
pub const MAX_ZOOM: u8 = 27;

/// The order a sink writes tiles in; ascending tile ids follow it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TileOrder {
    /// `PMTiles` v3 Hilbert ids, so the archive is written clustered.
    Hilbert,
    /// Zoom, column, then bottom-up TMS row: the `(zoom_level, tile_column, tile_row)` primary key of `MBTiles`,
    /// so inserts append to its B-tree instead of splitting pages.
    Tms,
}

impl TileOrder {
    pub fn tile_id(self, coord: TileCoord) -> TileGenResult<u64> {
        let (z, x, y) = (coord.z(), coord.x(), coord.y());
        if z > MAX_ZOOM || !TileCoord::is_possible_on_zoom_level(z, x, y) {
            return Err(TileGenError::InvalidTile(coord));
        }
        Ok(match self {
            Self::Hilbert => TileId::from(pmtiles::TileCoord::new(z, x, y)?).value(),
            Self::Tms => pyramid_base(z) + (u64::from(x) << z) + u64::from(flip_row(z, y)),
        })
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "x and row are below 2^MAX_ZOOM"
    )]
    pub fn tile_coord(self, id: u64) -> TileGenResult<TileCoord> {
        if id >= pyramid_base(MAX_ZOOM + 1) {
            return Err(TileGenError::InvalidTileId(id));
        }
        Ok(match self {
            Self::Hilbert => {
                let coord = pmtiles::TileCoord::from(TileId::new(id)?);
                TileCoord::new_unchecked(coord.z(), coord.x(), coord.y())
            }
            Self::Tms => {
                let z = zoom_of(id);
                let offset = id - pyramid_base(z);
                let row = (offset & ((1 << z) - 1)) as u32;
                TileCoord::new_unchecked(z, (offset >> z) as u32, flip_row(z, row))
            }
        })
    }
}

impl TileOrder {
    /// Appends contiguous id ranges that together hold exactly the tiles `x` × `y` (XYZ) of `zoom`,
    /// merging adjacent ones. A covered rectangle then costs a range per column (TMS) or per aligned
    /// quadtree cell (Hilbert, whose cells are contiguous along the curve), not a record per tile.
    pub fn fill_ranges(
        self,
        zoom: u8,
        x: Range<u32>,
        y: Range<u32>,
        out: &mut Vec<Range<u64>>,
    ) -> TileGenResult<()> {
        let side = 1u32 << zoom.min(MAX_ZOOM);
        let (x, y) = (x.start..x.end.min(side), y.start..y.end.min(side));
        if zoom > MAX_ZOOM || x.is_empty() || y.is_empty() {
            return Ok(());
        }
        let first = out.len();
        match self {
            Self::Tms => {
                for col in x {
                    let start = self.tile_id(TileCoord::new_unchecked(zoom, col, y.end - 1))?;
                    out.push(start..start + u64::from(y.end - y.start));
                }
            }
            Self::Hilbert => self.hilbert_cells(zoom, 0, (0, 0), (&x, &y), out)?,
        }
        let ranges = &mut out[first..];
        ranges.sort_unstable_by_key(|r| r.start);
        let mut merged = first;
        for i in first..out.len() {
            if merged > first && out[merged - 1].end == out[i].start {
                out[merged - 1].end = out[i].end;
            } else {
                out[merged] = out[i].clone();
                merged += 1;
            }
        }
        out.truncate(merged);
        Ok(())
    }

    /// The cell `(cx, cy)` of `level` covers `2^(zoom - level)` tiles per side of `zoom`; its tiles are one
    /// contiguous Hilbert range, starting at the cell's own index scaled to the finer zoom.
    fn hilbert_cells(
        self,
        zoom: u8,
        level: u8,
        (cx, cy): (u32, u32),
        (x, y): (&Range<u32>, &Range<u32>),
        out: &mut Vec<Range<u64>>,
    ) -> TileGenResult<()> {
        let shift = zoom - level;
        let (x0, y0) = (cx << shift, cy << shift);
        let (x1, y1) = (x0 + (1 << shift), y0 + (1 << shift));
        if x1 <= x.start || x0 >= x.end || y1 <= y.start || y0 >= y.end {
            return Ok(());
        }
        if x0 >= x.start && x1 <= x.end && y0 >= y.start && y1 <= y.end {
            let cell = self.tile_id(TileCoord::new_unchecked(level, cx, cy))? - pyramid_base(level);
            let start = pyramid_base(zoom) + (cell << (2 * shift));
            out.push(start..start + (1 << (2 * shift)));
            return Ok(());
        }
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            self.hilbert_cells(zoom, level + 1, (cx * 2 + dx, cy * 2 + dy), (x, y), out)?;
        }
        Ok(())
    }
}

/// Number of tiles in all zooms below `z`, i.e. the first id of zoom `z`.
fn pyramid_base(z: u8) -> u64 {
    PYRAMID_SIZE_BY_ZOOM[usize::from(z)]
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "at most MAX_ZOOM + 1 bases"
)]
fn zoom_of(id: u64) -> u8 {
    let bases = &PYRAMID_SIZE_BY_ZOOM[..=usize::from(MAX_ZOOM)];
    (bases.partition_point(|&base| base <= id) - 1) as u8
}

/// Converts between XYZ (top-down) and TMS (bottom-up) rows; the conversion is its own inverse.
fn flip_row(z: u8, row: u32) -> u32 {
    (1 << z) - 1 - row
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn all_tiles(max_zoom: u8) -> impl Iterator<Item = TileCoord> {
        (0..=max_zoom).flat_map(|z| {
            (0..1 << z)
                .flat_map(move |x| (0..1 << z).map(move |y| TileCoord::new_unchecked(z, x, y)))
        })
    }

    #[rstest]
    fn round_trip(#[values(TileOrder::Hilbert, TileOrder::Tms)] order: TileOrder) {
        let max = (1 << MAX_ZOOM) - 1;
        let corners = [(0, 0), (max, 0), (0, max), (max, max)]
            .map(|(x, y)| TileCoord::new_unchecked(MAX_ZOOM, x, y));
        for coord in all_tiles(5).chain(corners) {
            let id = order.tile_id(coord).unwrap();
            assert!(
                id < 1 << 56,
                "{coord:#} id {id} leaves no room for the layer"
            );
            assert_eq!(order.tile_coord(id).unwrap(), coord);
        }
    }

    #[test]
    fn hilbert_ids_match_pmtiles() {
        for coord in all_tiles(5) {
            let expected =
                TileId::from(pmtiles::TileCoord::new(coord.z(), coord.x(), coord.y()).unwrap());
            assert_eq!(TileOrder::Hilbert.tile_id(coord).unwrap(), expected.value());
        }
    }

    #[test]
    fn tms_ids_follow_mbtiles_primary_key() {
        let mut by_id: Vec<_> = all_tiles(4).collect();
        by_id.sort_by_key(|&c| TileOrder::Tms.tile_id(c).unwrap());
        let mut by_key = by_id.clone();
        by_key.sort_by_key(|c| (c.z(), c.x(), flip_row(c.z(), c.y())));
        assert_eq!(by_id, by_key);
    }

    #[rstest]
    #[case::zoom_above_max(MAX_ZOOM + 1, 0, 0)]
    #[case::x_outside(3, 8, 0)]
    #[case::y_outside(3, 0, 8)]
    fn rejects_invalid_tiles(
        #[values(TileOrder::Hilbert, TileOrder::Tms)] order: TileOrder,
        #[case] z: u8,
        #[case] x: u32,
        #[case] y: u32,
    ) {
        let coord = TileCoord::new_unchecked(z, x, y);
        assert!(matches!(order.tile_id(coord), Err(TileGenError::InvalidTile(c)) if c == coord));
    }

    #[rstest]
    fn fill_ranges_hold_exactly_the_rectangle(
        #[values(TileOrder::Hilbert, TileOrder::Tms)] order: TileOrder,
    ) {
        let mut ranges = Vec::new();
        for (zoom, x, y) in [
            (0, 0..1, 0..1),
            (3, 0..8, 0..8),
            (3, 1..6, 2..7),
            (4, 5..6, 0..16),
            (4, 3..11, 9..10),
            (5, 7..30, 3..19),
        ] {
            ranges.clear();
            order
                .fill_ranges(zoom, x.clone(), y.clone(), &mut ranges)
                .unwrap();
            let mut got: Vec<u64> = ranges.iter().flat_map(Clone::clone).collect();
            let mut want: Vec<u64> = x
                .clone()
                .flat_map(|tx| y.clone().map(move |ty| (tx, ty)))
                .map(|(tx, ty)| {
                    order
                        .tile_id(TileCoord::new_unchecked(zoom, tx, ty))
                        .unwrap()
                })
                .collect();
            got.sort_unstable();
            want.sort_unstable();
            assert_eq!(got, want, "z{zoom} {x:?} x {y:?}");
            assert!(
                ranges.windows(2).all(|w| w[0].end < w[1].start),
                "merged and sorted"
            );
        }
        ranges.clear();
        order.fill_ranges(5, 0..32, 0..32, &mut ranges).unwrap();
        assert_eq!(ranges.len(), 1, "a whole zoom is one range");
    }

    #[rstest]
    fn rejects_ids_above_max_zoom(#[values(TileOrder::Hilbert, TileOrder::Tms)] order: TileOrder) {
        let first_invalid = pyramid_base(MAX_ZOOM + 1);
        assert_eq!(order.tile_coord(first_invalid - 1).unwrap().z(), MAX_ZOOM);
        assert!(matches!(
            order.tile_coord(first_invalid),
            Err(TileGenError::InvalidTileId(id)) if id == first_invalid
        ));
    }
}

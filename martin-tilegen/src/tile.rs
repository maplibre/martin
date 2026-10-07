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
    fn rejects_ids_above_max_zoom(#[values(TileOrder::Hilbert, TileOrder::Tms)] order: TileOrder) {
        let first_invalid = pyramid_base(MAX_ZOOM + 1);
        assert_eq!(order.tile_coord(first_invalid - 1).unwrap().z(), MAX_ZOOM);
        assert!(matches!(
            order.tile_coord(first_invalid),
            Err(TileGenError::InvalidTileId(id)) if id == first_invalid
        ));
    }
}

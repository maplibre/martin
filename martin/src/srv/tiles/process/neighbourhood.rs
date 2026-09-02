//! Reading the 3x3 tile neighbourhood a stitching processor needs.
//!
//! Shared by every pass whose kernel samples past a tile's own edge, so they
//! agree on how the map wraps and where it stops.

use std::sync::{Arc, LazyLock};

use futures::stream::{self, StreamExt as _};
use martin_core::tiles::neighbourhood::{NEIGHBOURHOOD_LEN, Neighbourhood};
use martin_core::tiles::{BoxedSource, MartinCoreError, Tile, TileCache, TileCacheKey};
use martin_tile_utils::{TileCoord, TileData};
use tokio::sync::Semaphore;
use tracing::debug;

/// Bounds concurrent neighbourhood gathers across the process.
const MAX_CONCURRENT_GATHERS: usize = 8;

/// Bounds concurrent neighbourhood gathers across the process.
pub static GATHER_PERMITS: LazyLock<Semaphore> =
    LazyLock::new(|| Semaphore::new(MAX_CONCURRENT_GATHERS));

/// Tile bytes and the etag identifying them.
pub type Slot = Option<(TileData, String)>;

/// Coordinate of the neighbour `(dx, dy)` away from `centre`, if one exists.
///
/// On a grid that `wraps` (Web Mercator) x is cylindrical, so a tile at the antimeridian has real neighbours on its far side and x wraps rather than clamping.
/// y never wraps, and neither axis does on any other grid.
/// Past such an edge there is no tile at all, so the slot is left empty and the assembler edge-clamps it.
pub fn neighbour_coord(centre: TileCoord, dx: i32, dy: i32, wraps: bool) -> Option<TileCoord> {
    let side = 1i64 << centre.z;
    let x = i64::from(centre.x) + i64::from(dx);
    let x = if wraps {
        x.rem_euclid(side)
    } else if (0..side).contains(&x) {
        x
    } else {
        return None;
    };
    let y = i64::from(centre.y) + i64::from(dy);
    if y < 0 || y >= side {
        return None;
    }
    // Both coordinates are now within `side`, which is what makes this valid.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "x is reduced mod side and y is bounds-checked above"
    )]
    Some(TileCoord::new_unchecked(centre.z, x as u32, y as u32))
}

/// Reads one tile as its source produced it.
async fn fetch_raw(
    source: &BoxedSource,
    xyz: TileCoord,
    cache: Option<&TileCache>,
) -> Result<Tile, Arc<MartinCoreError>> {
    let src = source.clone_source();
    let compute = || async move { src.get_tile_with_etag(xyz, None).await };

    let cacheable = source.cache_zoom().contains(xyz.z);
    match (cache, cacheable) {
        (Some(cache), true) => {
            cache
                .get_or_insert(
                    TileCacheKey::new_request_static(source.get_id().to_owned(), xyz),
                    compute,
                )
                .await
        }
        _ => compute().await.map_err(Arc::new),
    }
}

/// Reads the 3x3 neighbourhood centred on `xyz`.
///
/// Only a failed *centre* is an error: a missing or failed neighbour degrades
/// one seam, which the assembler covers by clamping the centre's edge over it.
pub async fn gather(
    source: &BoxedSource,
    xyz: TileCoord,
    cache: Option<&TileCache>,
) -> Result<[Slot; NEIGHBOURHOOD_LEN], Arc<MartinCoreError>> {
    let wraps = source.tile_grid().wraps();
    let reads = (0..NEIGHBOURHOOD_LEN).map(|index| async move {
        let (dx, dy) = Neighbourhood::offset(index);
        let Some(coord) = neighbour_coord(xyz, dx, dy, wraps) else {
            return (index, Ok(None));
        };
        let result = fetch_raw(source, coord, cache).await;
        (index, result.map(Some))
    });

    // All nine at once since they are independent, and the gather as a whole is already bounded process-wide by GATHER_PERMITS.
    let mut collected = stream::iter(reads).buffer_unordered(NEIGHBOURHOOD_LEN);

    let mut slots: [Slot; NEIGHBOURHOOD_LEN] = Default::default();
    while let Some((index, result)) = collected.next().await {
        match result {
            Ok(Some(tile)) => {
                // An empty tile is a coverage hole, not data; clamp over it.
                if !tile.data.is_empty() {
                    slots[index] = Some((tile.data, tile.etag));
                }
            }
            Ok(None) => {}
            Err(e) if index == Neighbourhood::CENTRE => return Err(e),
            Err(e) => {
                debug!(
                    source.id = source.get_id(),
                    slot = index,
                    error = %e,
                    "Neighbour tile unavailable. clamping the centre's edge over it"
                );
            }
        }
    }
    Ok(slots)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::interior_is_offset(4, 8, 8, -1, -1, true, Some((7, 7)))]
    #[case::interior_se_is_offset(4, 8, 8, 1, 1, true, Some((9, 9)))]
    #[case::wrap_west_cylindrically(2, 0, 2, -1, 0, true, Some((3, 2)))]
    #[case::wrap_east_cylindrically(2, 3, 2, 1, 0, true, Some((0, 2)))]
    #[case::past_north_pole(2, 1, 0, 0, -1, true, None)]
    #[case::past_south_pole(2, 1, 3, 0, 1, true, None)]
    #[case::zoom_zero_wraps(0, 0, 0, 1, 0, true, Some((0, 0)))]
    #[case::zoom_zero_no_row(0, 0, 0, 0, 1, true, None)]
    #[case::planar_interior_is_offset(4, 8, 8, -1, -1, false, Some((7, 7)))]
    #[case::planar_west_edge_stops(2, 0, 2, -1, 0, false, None)]
    #[case::planar_east_edge_stops(2, 3, 2, 1, 0, false, None)]
    #[case::planar_zoom_zero_has_no_neighbours(0, 0, 0, 1, 0, false, None)]
    fn neighbour_coordinates_wrap_in_x_only_on_wrapping_grids(
        #[case] z: u8,
        #[case] x: u32,
        #[case] y: u32,
        #[case] dx: i32,
        #[case] dy: i32,
        #[case] wraps: bool,
        #[case] expected: Option<(u32, u32)>,
    ) {
        let got = neighbour_coord(TileCoord::new_unchecked(z, x, y), dx, dy, wraps);
        assert_eq!(got.map(|c| (c.x, c.y)), expected);
        if let Some(coord) = got {
            assert_eq!(coord.z, z, "a neighbour is always at the same zoom");
        }
    }

    #[test]
    fn the_centre_offset_is_the_tile_itself() {
        let xyz = TileCoord::new_unchecked(5, 10, 12);
        let (dx, dy) = Neighbourhood::offset(Neighbourhood::CENTRE);
        assert_eq!(neighbour_coord(xyz, dx, dy, true), Some(xyz));
    }

    #[test]
    fn every_offset_resolves_for_an_interior_tile() {
        let xyz = TileCoord::new_unchecked(6, 30, 30);
        let resolved: Vec<_> = (0..NEIGHBOURHOOD_LEN)
            .map(Neighbourhood::offset)
            .filter_map(|(dx, dy)| neighbour_coord(xyz, dx, dy, false))
            .collect();
        assert_eq!(resolved.len(), NEIGHBOURHOOD_LEN);
        // All nine are distinct, so the pass reads nine different tiles.
        let mut unique = resolved.clone();
        unique.sort_unstable_by_key(|c| (c.x, c.y));
        unique.dedup_by_key(|c| (c.x, c.y));
        assert_eq!(unique.len(), NEIGHBOURHOOD_LEN);
    }
}

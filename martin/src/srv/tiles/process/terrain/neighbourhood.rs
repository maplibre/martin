//! Reading the 3x3 tile neighbourhood a stitching processor needs.
//!
//! Shared by every pass whose kernel samples past a tile's own edge, so they
//! agree on how the map wraps and where it stops.

use std::sync::{Arc, LazyLock};

use compact_str::CompactString;
use futures::stream::{self, StreamExt as _};
use martin_core::tiles::neighbourhood::{AncestorWindow, NEIGHBOURHOOD_LEN, Neighbourhood};
use martin_core::tiles::{BoxedSource, MartinCoreError, Tile, TileCache, TileCacheKey};
use martin_tile_utils::{TileCoord, TileData, TileGrid};
use tokio::sync::Semaphore;
use tracing::debug;

/// Bounds concurrent neighbourhood gathers across the process.
const MAX_CONCURRENT_GATHERS: usize = 8;

/// Bounds concurrent neighbourhood gathers across the process.
pub static GATHER_PERMITS: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(MAX_CONCURRENT_GATHERS));

/// Tile bytes and the etag identifying them.
pub type Slot = Option<(TileData, CompactString)>;

/// Coordinate of the neighbour `(dx, dy)` away from `centre`, if one exists.
///
/// On a grid that wraps (the built-in ones) x is cylindrical, so a tile at the antimeridian has real neighbours on its far side and x wraps rather than clamping.
/// y never wraps, and neither axis does on any other grid.
/// Past such an edge there is no tile at all, so the slot is left empty and the assembler edge-clamps it.
pub fn neighbour_coord(centre: TileCoord, dx: i32, dy: i32, grid: &TileGrid) -> Option<TileCoord> {
    let [columns, rows] = grid.matrix_at_zoom0();
    let width = i64::from(columns) << centre.z();
    let height = i64::from(rows) << centre.z();
    let x = i64::from(centre.x()) + i64::from(dx);
    let x = if grid.wraps() {
        x.rem_euclid(width)
    } else if (0..width).contains(&x) {
        x
    } else {
        return None;
    };
    let y = i64::from(centre.y()) + i64::from(dy);
    if !(0..height).contains(&y) {
        return None;
    }
    // Both coordinates are now within the grid, which is what makes this valid.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "x is wrapped or bounds-checked and y is bounds-checked above"
    )]
    Some(TileCoord::new_unchecked(centre.z(), x as u32, y as u32))
}

/// Reads one tile as its source produced it.
async fn fetch_raw(
    source: &BoxedSource,
    xyz: TileCoord,
    cache: Option<&TileCache>,
) -> Result<Tile, Arc<MartinCoreError>> {
    let src = Arc::clone(source);
    let compute = || async move { src.get_tile_with_etag(xyz, None).await };

    let cacheable = source.cache_zoom().contains(xyz.z());
    match (cache, cacheable) {
        (Some(cache), true) => {
            cache
                .get_or_insert(TileCacheKey::new_request_static(source.get_id(), xyz), compute)
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
    gather_around(source, xyz, cache, None).await
}

/// A neighbourhood read for a requested tile, and where that tile sits inside its centre.
pub struct Gathered {
    /// Where the requested tile sits inside the centre slot.
    pub window: AncestorWindow,
    /// The nine slots, in row-major order.
    pub slots: [Slot; NEIGHBOURHOOD_LEN],
}

/// Reads the 3x3 neighbourhood for `xyz`, centred on its nearest ancestor that holds data when the source does not hold `xyz` itself.
///
/// A source with variable-depth coverage, such as a DEM that only reaches its highest zooms in some regions, answers an empty tile where it stops.
/// The walk stops at the source's minzoom.
/// When no zoom down to it holds data, the slots read at `xyz` come back with an empty centre.
pub async fn gather_nearest(
    source: &BoxedSource,
    xyz: TileCoord,
    cache: Option<&TileCache>,
) -> Result<Gathered, Arc<MartinCoreError>> {
    let slots = gather(source, xyz, cache).await?;
    if slots[Neighbourhood::CENTRE].is_some() {
        return Ok(Gathered {
            window: AncestorWindow::NATIVE,
            slots,
        });
    }
    let floor = source.get_tilejson().minzoom.unwrap_or(0);
    for levels in 1..=xyz.z().saturating_sub(floor) {
        let (ancestor, window) = AncestorWindow::above(xyz, levels);
        let centre = fetch_raw(source, ancestor, cache).await?;
        if centre.data.is_empty() {
            continue;
        }
        debug!(
            source.id = source.get_id(),
            tile = %xyz,
            ancestor = %ancestor,
            "No elevation tile at this zoom; reading its nearest ancestor instead"
        );
        let slots = gather_around(source, ancestor, cache, Some((centre.data, centre.etag))).await?;
        return Ok(Gathered { window, slots });
    }
    Ok(Gathered {
        window: AncestorWindow::NATIVE,
        slots,
    })
}

/// Reads the 3x3 neighbourhood centred on `xyz`, reusing `centre` instead of reading it again when the caller already has it.
async fn gather_around(
    source: &BoxedSource,
    xyz: TileCoord,
    cache: Option<&TileCache>,
    centre: Slot,
) -> Result<[Slot; NEIGHBOURHOOD_LEN], Arc<MartinCoreError>> {
    let known_centre = centre.is_some();
    let grid = source.tile_grid();
    let reads = (0..NEIGHBOURHOOD_LEN)
        .filter(|&index| !(known_centre && index == Neighbourhood::CENTRE))
        .map(|index| async move {
            let (dx, dy) = Neighbourhood::offset(index);
            let Some(coord) = neighbour_coord(xyz, dx, dy, grid) else {
                return (index, Ok(None));
            };
            let result = fetch_raw(source, coord, cache).await;
            (index, result.map(Some))
        });

    // All nine at once since they are independent, and the gather as a whole is already bounded process-wide by GATHER_PERMITS.
    let mut collected = stream::iter(reads).buffer_unordered(NEIGHBOURHOOD_LEN);

    let mut slots: [Slot; NEIGHBOURHOOD_LEN] = Default::default();
    slots[Neighbourhood::CENTRE] = centre;
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
    use martin_tile_utils::{TileGrid, WEB_MERCATOR_QUAD, WORLD_CRS84_QUAD};
    use rstest::rstest;

    use super::*;

    /// A grid that neither wraps nor has geographic meaning.
    fn planar() -> TileGrid {
        TileGrid::new("plan", martin_tile_utils::SIMPLE_CRS, [0.0, 1024.0], 1024.0).unwrap()
    }

    #[rstest]
    #[case::interior_is_offset(&WEB_MERCATOR_QUAD, 4, 8, 8, -1, -1, Some((7, 7)))]
    #[case::interior_se_is_offset(&WEB_MERCATOR_QUAD, 4, 8, 8, 1, 1, Some((9, 9)))]
    #[case::wrap_west_cylindrically(&WEB_MERCATOR_QUAD, 2, 0, 2, -1, 0, Some((3, 2)))]
    #[case::wrap_east_cylindrically(&WEB_MERCATOR_QUAD, 2, 3, 2, 1, 0, Some((0, 2)))]
    #[case::past_north_pole(&WEB_MERCATOR_QUAD, 2, 1, 0, 0, -1, None)]
    #[case::past_south_pole(&WEB_MERCATOR_QUAD, 2, 1, 3, 0, 1, None)]
    #[case::zoom_zero_wraps(&WEB_MERCATOR_QUAD, 0, 0, 0, 1, 0, Some((0, 0)))]
    #[case::zoom_zero_no_row(&WEB_MERCATOR_QUAD, 0, 0, 0, 0, 1, None)]
    #[case::two_wide_wraps_at_zoom_zero(&WORLD_CRS84_QUAD, 0, 1, 0, 1, 0, Some((0, 0)))]
    #[case::two_wide_has_one_row_at_zoom_zero(&WORLD_CRS84_QUAD, 0, 1, 0, 0, 1, None)]
    #[case::two_wide_east_edge_at_zoom_one(&WORLD_CRS84_QUAD, 1, 3, 1, 1, 0, Some((0, 1)))]
    #[case::planar_interior_is_offset(&planar(), 4, 8, 8, -1, -1, Some((7, 7)))]
    #[case::planar_west_edge_stops(&planar(), 2, 0, 2, -1, 0, None)]
    #[case::planar_east_edge_stops(&planar(), 2, 3, 2, 1, 0, None)]
    #[case::planar_zoom_zero_has_no_neighbours(&planar(), 0, 0, 0, 1, 0, None)]
    fn neighbour_coordinates_follow_the_grid(
        #[case] grid: &TileGrid,
        #[case] z: u8,
        #[case] x: u32,
        #[case] y: u32,
        #[case] dx: i32,
        #[case] dy: i32,
        #[case] expected: Option<(u32, u32)>,
    ) {
        let got = neighbour_coord(TileCoord::new_unchecked(z, x, y), dx, dy, grid);
        assert_eq!(got.map(|c| (c.x(), c.y())), expected);
        if let Some(coord) = got {
            assert_eq!(coord.z(), z, "a neighbour is always at the same zoom");
        }
    }

    #[test]
    fn the_centre_offset_is_the_tile_itself() {
        let xyz = TileCoord::new_unchecked(5, 10, 12);
        let (dx, dy) = Neighbourhood::offset(Neighbourhood::CENTRE);
        assert_eq!(neighbour_coord(xyz, dx, dy, &WEB_MERCATOR_QUAD), Some(xyz));
    }

    #[test]
    fn every_offset_resolves_for_an_interior_tile() {
        let xyz = TileCoord::new_unchecked(6, 30, 30);
        let resolved: Vec<_> = (0..NEIGHBOURHOOD_LEN)
            .map(Neighbourhood::offset)
            .filter_map(|(dx, dy)| neighbour_coord(xyz, dx, dy, &planar()))
            .collect();
        assert_eq!(resolved.len(), NEIGHBOURHOOD_LEN);
        // All nine are distinct, so the pass reads nine different tiles.
        let mut unique = resolved;
        unique.sort_unstable_by_key(|c| (c.x(), c.y()));
        unique.dedup_by_key(|c| (c.x(), c.y()));
        assert_eq!(unique.len(), NEIGHBOURHOOD_LEN);
    }
}

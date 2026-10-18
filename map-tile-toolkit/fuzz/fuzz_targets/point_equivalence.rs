//! Fuzz the point slicer: it must **never panic**, and a point it slices without error must agree with
//! the single-tile call and with a direct test of every candidate tile.
//!
//! For a structured input (a slicer config, some points, and an arbitrary probe tile):
//!
//! 1. **No panic, ever.** `Slicer::tile_point` on an arbitrary (possibly extreme) tile, and
//!    `Slicer::point`, must return `Ok`/`Err` — never panic or overflow. The fuzz build enables
//!    overflow checks, so unchecked arithmetic surfaces here as a crash.
//! 2. When `Slicer::point` returns no error, then for each of the nine tiles around the point's own,
//!    `Slicer::tile_point` holds the point exactly when `point` yielded that tile (with the same local
//!    position), and `point` yields nothing else: its own tile always, and a neighbor only within
//!    `buffer` of the shared edge.
//!
//! Coordinates start as `i8`, then are scaled by a power of two and moved by an offset, so the tile
//! math also runs on large values and near the `i32` limits.

#![no_main]

use arbitrary::Arbitrary;
use geo_types::Coord;
use libfuzzer_sys::fuzz_target;
use map_tile_toolkit::{Slicer, TileId};

#[derive(Arbitrary, Debug)]
struct Input {
    extent: u32,
    buffer: u16,
    /// Coordinates are multiplied by `2^(scale % 24)`.
    scale: u8,
    /// Added to every coordinate after scaling (wrapping, so points can sit at the `i32` limits).
    origin: (i32, i32),
    points: Vec<(i8, i8)>,
    /// An arbitrary tile to probe with the single-tile call.
    probe: (i32, i32),
}

fuzz_target!(|input: Input| {
    let Ok(slicer) = Slicer::<Coord<i32>>::new(input.extent, input.buffer) else {
        return; // an invalid config is rejected — nothing to test
    };
    let scale = 1_i32 << (input.scale % 24);
    let extent = i64::from(input.extent);
    let (b, e) = (i64::from(input.buffer), extent);
    let probe = TileId::new(input.probe.0, input.probe.1);

    for &(x, y) in input.points.iter().take(64) {
        let point = Coord {
            x: i32::from(x).wrapping_mul(scale).wrapping_add(input.origin.0),
            y: i32::from(y).wrapping_mul(scale).wrapping_add(input.origin.1),
        };

        // (1) A single-tile call on an arbitrary tile never panics.
        let _ = slicer.tile_point(probe, point);

        // The all-tiles call never panics; an error is a valid outcome for points near the limits.
        let Ok(slices) = slicer.point(point) else {
            continue;
        };
        let mut yielded = Vec::new();
        for slice in slices {
            let local = slice.point();
            let origin = slice
                .tile()
                .origin(input.extent)
                .expect("a yielded tile is in range");
            // The local position is the global one minus the tile origin, and the point is inside
            // the tile's buffered box.
            assert_eq!(local + origin, point, "local position");
            for v in [local.x, local.y] {
                assert!(
                    i64::from(v) >= -b && i64::from(v) < e + b,
                    "{local:?} outside the buffered box"
                );
            }
            assert!(!yielded.contains(&slice.tile()), "yielded twice");
            yielded.push(slice.tile());
        }

        // (2) Each of the nine tiles around the point's own agrees with `tile_point`.
        let (tx, ty) = (
            i64::from(point.x).div_euclid(extent),
            i64::from(point.y).div_euclid(extent),
        );
        for (dx, dy) in (-1..=1).flat_map(|dx| (-1..=1).map(move |dy| (dx, dy))) {
            let (Ok(x), Ok(y)) = (i32::try_from(tx + dx), i32::try_from(ty + dy)) else {
                continue;
            };
            let tile = TileId::new(x, y);
            // A tile whose box overflows cannot be asked; `point` did not error, so it holds nothing.
            let direct = slicer.tile_point(tile, point).ok().flatten();
            assert_eq!(yielded.contains(&tile), direct.is_some(), "{point:?} in {tile:?}");
        }
        // Nothing yielded outside those nine tiles.
        for tile in &yielded {
            assert!(
                (i64::from(tile.x) - tx).abs() <= 1 && (i64::from(tile.y) - ty).abs() <= 1,
                "{point:?} yielded for far tile {tile:?}"
            );
        }
    }
});

//! The slicers are lazy and borrow their input. Lines and points allocate **nothing**, however long
//! the feature; polygons keep only one row of one polygon, a row costing memory per edge, and once a
//! slicer is warmed up allocate nothing either. A counting global allocator proves it on a 200 000-vertex
//! line, a long diagonal across tens of thousands of tiles, a z14 ocean, and a coastline-like polygon.

#![allow(clippy::pedantic, reason = "test tool")]

use std::f64::consts::TAU;

use geo_types::Coord;
use map_tile_toolkit::{Slicer, TileId};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

mod support;

#[global_allocator]
static GLOBAL: &StatsAlloc<std::alloc::System> = &INSTRUMENTED_SYSTEM;

/// Allocations made while `f` runs (the test binary holds this one test, so nothing else runs).
fn allocations(f: impl FnOnce()) -> usize {
    let region = Region::new(GLOBAL);
    f();
    region.change().allocations
}

/// One test, so no other test's allocations land in a measured window (the counter is process-wide).
#[test]
fn slicing_allocates_little() {
    lines_and_points_allocate_nothing();
    polygon_memory_is_one_row();
    polygon_index_is_a_fraction_of_the_input();
}

fn lines_and_points_allocate_nothing() {
    // A zigzag of 200 000 vertices across a 40 × 40-tile area, and a single segment across
    // 40 000 × 40 000 tiles.
    let zigzag: Vec<Coord<i32>> = (0..200_000)
        .map(|i| Coord {
            x: (i * 37) % 1000,
            y: (i * 91) % 1000,
        })
        .collect();
    let diagonal = vec![
        Coord { x: 3, y: 5 },
        Coord {
            x: 999_990,
            y: 999_000,
        },
    ];
    let points: Vec<Coord<i32>> = zigzag.iter().step_by(7).copied().collect();
    let slicer = Slicer::new(25, 2).expect("config");
    let lines = [zigzag.as_slice(), diagonal.as_slice()];

    let mut slices = 0u64;
    let mut vertices = 0u64;
    let mut slice_all = || {
        for line in lines {
            for s in slicer.line(line).expect("line") {
                slices += 1;
                vertices += s.run().iter().count() as u64;
            }
            for s in slicer.tile_line(TileId::new(3, 4), line).expect("tile") {
                vertices += s.run().iter().count() as u64;
            }
        }
        for &point in &points {
            for _ in slicer.point(point).expect("point") {
                slices += 1;
            }
        }
    };
    // A warm-up pass first, for the one-time setup of the `hotpath` profiler when it is enabled.
    slice_all();
    let used = allocations(&mut slice_all);
    assert!(slices > 100_000, "{slices}");
    assert!(vertices > 200_000, "{vertices}");
    assert_eq!(used, 0, "slicing allocated {used} times");
}

/// Count the bytes allocated while `f` runs.
fn bytes(f: impl FnOnce()) -> usize {
    let region = Region::new(GLOBAL);
    f();
    region.change().bytes_allocated
}

/// Drain a polygon's slices, reading every vertex; the number of slices.
fn drain(slicer: &mut Slicer<Coord<i32>>, input: &[&[Coord<i32>]]) -> u64 {
    let mut slices = slicer.polygon(input).expect("polygon");
    let mut n = 0;
    while let Some(slice) = slices.next() {
        if let map_tile_toolkit::PolygonSlice::Piece(piece) = slice.expect("slice") {
            for ring in piece.rings() {
                for v in ring {
                    std::hint::black_box(v);
                }
            }
        }
        n += 1;
    }
    n
}

fn polygon_memory_is_one_row() {
    // A z14 ocean: 2^28 covered tiles and edges reaching 16 384 tiles each. A row costs memory per
    // edge, not per tile, so its first slicing allocates under a kilobyte or so; once warmed up,
    // slicing allocates nothing.
    let ocean = support::huge_square();
    let input = support::rings(&ocean);
    let mut slicer = Slicer::new(4096, 64).expect("config");
    let mut slices = 0;
    let first = bytes(|| slices = drain(&mut slicer, &input));
    assert!(slices > 16_000, "{slices}");
    assert!(first < 4 * 1024, "first slicing allocated {first} bytes");
    assert_eq!(
        allocations(|| {
            drain(&mut slicer, &input);
        }),
        0
    );

    // A long coastline-like polygon (the big fixture polygon) at a fine grid: likewise.
    let big = support::big_polygon();
    let input = support::rings(&big);
    let mut slicer = Slicer::new(25, 2).expect("config");
    drain(&mut slicer, &input);
    assert_eq!(
        allocations(|| {
            drain(&mut slicer, &input);
        }),
        0
    );
}

/// A jagged 200 000-vertex ring, a coastline-like polygon spanning thousands of tiles. Its first
/// slicing allocates the row buffers and the index of its edges by row, together a few bytes per vertex
/// (cumulatively, counting every growth step) and under the 8 bytes per vertex the input itself takes;
/// once warmed up, slicing allocates nothing.
fn polygon_index_is_a_fraction_of_the_input() {
    const N: usize = 200_000;
    let ring: Vec<Coord<i32>> = (0..N)
        .map(|k| {
            let angle = TAU * k as f64 / N as f64;
            let radius = 3_000_000.0 + 20_000.0 * ((k * 37) % 50) as f64 / 50.0;
            Coord {
                x: (radius * angle.cos()) as i32 + 4_000_000,
                y: (radius * angle.sin()) as i32 + 4_000_000,
            }
        })
        .collect();
    let input = [ring.as_slice()];
    let mut slicer = Slicer::new(4096, 64).expect("config");
    let mut slices = 0;
    let first = bytes(|| slices = drain(&mut slicer, &input));
    assert!(slices > 10_000, "{slices}");
    assert!(
        first < 8 * N,
        "first slicing allocated {first} bytes for {N} vertices"
    );
    assert_eq!(
        allocations(|| {
            drain(&mut slicer, &input);
        }),
        0
    );
}

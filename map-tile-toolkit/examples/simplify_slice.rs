//! Load a file of WKT polylines in Web Mercator (EPSG:3857), simplify each one for a target zoom
//! level, project it onto the integer tile grid, and slice every geometry into its tiles with a
//! [`Slicer`]. Built as a `cargo flamegraph` target: parsing/simplification/projection all happen
//! up front, then a single tight loop does the slicing and [`black_box`]es the output so the profile
//! shows only the slicing cost.
//!
//! Usage: `cargo run --release --example simplify_slice -- <path-to.wkt> <zoom>`
//!
//! Pipeline:
//! * **load** - read the whole file into memory, one WKT `LINESTRING` per line.
//! * **simplify** - a pixel at `<zoom>` is `EARTH_CIRCUMFERENCE / 2^zoom / EXTENT` metres wide;
//!   run Ramer-Douglas-Peucker with an epsilon of ~3 pixels, in the source metre space.
//! * **project** - one tile at `<zoom>` spans `divisor = EARTH_CIRCUMFERENCE / 2^zoom` metres, cut
//!   into `EXTENT` units, so the whole world is `2^zoom * EXTENT` integer units across. Web Mercator
//!   metres map into that `i32` grid (Y flipped so tile row 0 is at the north edge).
//! * **slice** - slice every geometry with one [`Slicer`] (all lines share a single tile grid),
//!   reading each slice's tile and vertices through [`black_box`].

#![allow(clippy::pedantic, clippy::print_stderr, reason = "profiling helper")]

use std::collections::BTreeSet;
use std::hint::black_box;
use std::time::Instant;

use geo::Simplify as _;
use geo_types::{Coord, LineString};
use map_tile_toolkit::Slicer;
use wkt::TryFromWkt as _;

/// Web Mercator world circumference in metres (the EPSG:3857 axis range is `±HALF`).
const EARTH_CIRCUMFERENCE: f64 = 40_075_016.686;
/// Half the world span; the metre coordinate of the west / north edge is `-HALF`.
const HALF: f64 = EARTH_CIRCUMFERENCE / 2.0;
/// Per-tile output resolution: kept vertices land in `0..EXTENT`.
const EXTENT: u32 = 4096;
/// Simplify to roughly this many pixels of detail.
const SIMPLIFY_PIXELS: f64 = 3.0;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: simplify_slice <path-to.wkt> <zoom>");
    let zoom: u32 = args
        .next()
        .expect("usage: simplify_slice <path-to.wkt> <zoom>")
        .parse()
        .expect("zoom must be a non-negative integer");

    // One tile is `divisor` metres wide; a pixel is `divisor / EXTENT` metres.
    let tile_count = 2f64.powi(i32::try_from(zoom).expect("zoom fits i32"));
    let divisor = EARTH_CIRCUMFERENCE / tile_count;
    let pixel_m = divisor / f64::from(EXTENT);
    let epsilon = SIMPLIFY_PIXELS * pixel_m;
    // Metres -> integer grid units. World is `tile_count * EXTENT` units across.
    let units_per_m = f64::from(EXTENT) / divisor;
    eprintln!(
        "zoom {zoom}: divisor {divisor:.3} m/tile, pixel {pixel_m:.4} m, simplify epsilon {epsilon:.4} m"
    );

    // --- load: whole file into memory, parse each line as a WKT LINESTRING (f64, metres) ---
    let t = Instant::now();
    let text = std::fs::read_to_string(&path).expect("readable WKT file");
    let lines: Vec<LineString<f64>> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| LineString::try_from_wkt_str(l).expect("valid WKT LINESTRING"))
        .collect();
    eprintln!("loaded {} geometries in {:?}", lines.len(), t.elapsed());

    // --- simplify + project: RDP in metre space, then onto the integer tile grid ---
    let t = Instant::now();
    let geoms: Vec<LineString<i32>> = lines
        .iter()
        .map(|ls| {
            let simplified = ls.simplify(epsilon);
            let coords: Vec<Coord<i32>> = simplified
                .0
                .iter()
                .map(|c| Coord {
                    // X grows east from the west edge; Y grows south from the north edge.
                    x: ((c.x + HALF) * units_per_m) as i32,
                    y: ((HALF - c.y) * units_per_m) as i32,
                })
                .collect();
            LineString(coords)
        })
        .collect();
    let vertices: usize = geoms.iter().map(|ls| ls.0.len()).sum();
    eprintln!(
        "simplified + projected to {vertices} vertices in {:?}",
        t.elapsed()
    );

    // --- slice: the hot loop the flamegraph is meant to show ---
    let t = Instant::now();
    let mut skipped = 0u64;
    let mut pieces = 0u64;
    let mut seen = BTreeSet::new();
    // One slicer for every geometry: all lines share a single tile grid, the way you would build a
    // whole tiled dataset in one pass. Each slice is read as it comes (nothing is accumulated).
    let slicer = Slicer::new(EXTENT, 0).expect("valid slicer config");
    for geom in &geoms {
        let Ok(slices) = slicer.line(black_box(geom.0.as_slice())) else {
            skipped += 1;
            continue;
        };
        for slice in slices {
            seen.insert(black_box(slice.tile()));
            for v in slice.run() {
                black_box(v);
            }
            pieces += 1;
        }
    }
    let tiles = seen.len();
    black_box(pieces);
    eprintln!(
        "sliced {} geometries into {tiles} tiles / {pieces} pieces ({skipped} skipped) in {:?}",
        geoms.len(),
        t.elapsed()
    );
}

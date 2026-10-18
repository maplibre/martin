//! Minimal slicing driver for `hotpath` or `cargo flamegraph` — no measurement machinery, so the
//! profile shows only the slicing cost. Selects one case by name and runs it in a tight loop.
//!
//! Usage: `cargo run --release --example profile -- <name> [iterations]`, or `just toolkit::hotpath
//! <name> [iterations]`.
//!
//! Cases (`<name>`):
//! * `all` / `one` — slice the small fixture set into all tiles ([`Slicer::line`]) / per touched
//!   tile ([`Slicer::tile_line`]), reading every vertex of every slice.
//! * `big-all-<cfg>` / `big-one-<cfg>` — the large in-memory `big_polyline` sliced with the
//!   [`support::big_configs`] `<cfg>` (`multi`, `few`, or `single` — many / a few / one output tile).
//! * `poly-all-<cfg>` — the large `big_polygon` through one reused `Slicer::polygon` with the same
//!   `<cfg>`s, or `poly-all-huge` — the z14-like `huge_square` (2^28 tiles, nearly all covered).
//!
//! The second parameter is the iteration count (defaults chosen per scale). Tile ids for the `one`
//! cases are precomputed up front, so the loop measures only clipping/accumulation.

#![allow(clippy::pedantic, clippy::panic, reason = "profiling helper")]

use std::hint::black_box;

use geo_types::Coord;
use map_tile_toolkit::{LineSlice, TileId};

#[path = "../tests/support/mod.rs"]
mod support;

use support::Cfg;

/// Which operation to profile.
enum Op {
    All,
    One,
}

/// A polyline paired with the tile ids it touches (precomputed, not part of the hot loop).
type Case = (Vec<Coord<i32>>, Vec<TileId>);

/// The tiles a polyline touches, sorted (precomputed, not part of the hot loop).
fn touched_tiles(cfg: &Cfg, poly: &[Coord<i32>]) -> Vec<TileId> {
    support::slice_all_runs(cfg, &[poly.to_vec()])
        .into_iter()
        .map(|(tile, _)| tile)
        .collect()
}

/// Read every vertex of every slice, as a consumer encoding them would.
fn consume<'a>(slices: impl Iterator<Item = LineSlice<'a, Coord<i32>>>) {
    for s in slices {
        black_box(s.tile());
        for v in s.run() {
            black_box(v);
        }
    }
}

// With `--features hotpath`, `#[hotpath::main]` prints a per-function timing table on exit (the
// `#[hotpath::measure]`-annotated slicing functions); without it this is an ordinary `main`.
#[cfg_attr(feature = "hotpath", hotpath::main)]
fn main() {
    let name = std::env::args()
        .nth(1)
        .expect("usage: profile <name> [iterations]");
    let iterations: Option<u64> = std::env::args().nth(2).and_then(|s| s.parse().ok());
    if let Some(cfg) = name.strip_prefix("poly-all-") {
        profile_polygon(cfg, iterations);
        return;
    }
    let (polylines, cfg, op, default_iters) = resolve(&name);
    let iterations = iterations.unwrap_or(default_iters);

    // Precompute each polyline's tile ids once, outside the timed loop.
    let cases: Vec<Case> = polylines
        .into_iter()
        .map(|poly| {
            let tiles = touched_tiles(&cfg, &poly);
            (poly, tiles)
        })
        .collect();

    let slicer = cfg.slicer();
    for _ in 0..iterations {
        for (poly, tiles) in &cases {
            let poly = black_box(poly);
            match op {
                Op::All => consume(slicer.line(poly).expect("polyline")),
                Op::One => {
                    for &tile in tiles {
                        consume(slicer.tile_line(tile, poly).expect("polyline"));
                    }
                }
            }
        }
    }
}

/// Slice one large polygon repeatedly through a single slicer, reading every slice — the per-worker
/// pattern of a bulk tile generator.
fn profile_polygon(cfg: &str, iterations: Option<u64>) {
    let (polygon, slicer, default_iters) = if cfg == "huge" {
        (support::huge_square(), support::slicer(4096, 64), 30)
    } else {
        let slicer = support::big_configs()
            .into_iter()
            .find(|(c, _)| *c == cfg)
            .map(|(_, s)| s)
            .unwrap_or_else(|| panic!("unknown polygon config `{cfg}`"));
        (support::big_polygon(), slicer, 3_000)
    };
    let mut slicer = slicer.slicer();
    let input = support::rings(&polygon);
    for _ in 0..iterations.unwrap_or(default_iters) {
        let mut slices = slicer.polygon(black_box(&input)).expect("polygon");
        while let Some(slice) = slices.next() {
            match slice.expect("polygon") {
                map_tile_toolkit::PolygonSlice::Piece(piece) => {
                    for ring in piece.rings() {
                        for v in ring {
                            black_box(v);
                        }
                    }
                }
                map_tile_toolkit::PolygonSlice::Run(run) => {
                    black_box(run);
                }
            }
        }
    }
}

/// The component polylines of a geometry, owned.
fn polylines_of(geom: &geo_types::Geometry<i32>) -> Vec<Vec<Coord<i32>>> {
    support::lines_of(geom)
        .into_iter()
        .map(<[_]>::to_vec)
        .collect()
}

/// Resolve a case name into `(polylines, config, operation, default iterations)`.
fn resolve(name: &str) -> (Vec<Vec<Coord<i32>>>, Cfg, Op, u64) {
    let small = || {
        support::load_all_fixtures()
            .into_iter()
            .flat_map(|(_, polys)| polys)
            .collect()
    };
    match name {
        "all" => (small(), support::grid(), Op::All, 2_000_000),
        "one" => (small(), support::grid(), Op::One, 2_000_000),
        _ => {
            let rest = name
                .strip_prefix("big-")
                .unwrap_or_else(|| panic!("unknown case name: {name}"));
            let (op_str, cfg) = rest
                .split_once('-')
                .unwrap_or_else(|| panic!("unknown case name: {name}"));
            let op = match op_str {
                "all" => Op::All,
                "one" => Op::One,
                _ => panic!("unknown case name: {name}"),
            };
            let slicer = support::big_configs()
                .into_iter()
                .find(|(c, _)| *c == cfg)
                .map(|(_, s)| s)
                .unwrap_or_else(|| panic!("unknown big config `{cfg}` in: {name}"));
            (polylines_of(&support::big_polyline()), slicer, op, 3_000)
        }
    }
}

//! Fuzz the polyline slicer: it must **never panic**, and every `Ok` result must satisfy the
//! library's invariants.
//!
//! For a structured input (some polylines, a slicer config, and an arbitrary probe tile):
//!
//! 1. **No panic, ever.** `Slicer::tile_line` on an arbitrary (possibly extreme) tile, and
//!    `Slicer::line`, must return `Ok`/`Err` - never panic or overflow. The fuzz build enables
//!    overflow checks, so any unchecked arithmetic surfaces here as a crash.
//! 2. Per polyline, when `Slicer::line` returns no error, the results hold up:
//!    - **All-tiles round-trips through single-tile:** every tile's runs equal what
//!      `Slicer::tile_line` produces for that tile.
//!    - **All-tiles == exhaustive per-tile scan** over the reachable span (skipped when a tiny
//!      extent makes the span too large to scan cheaply).
//!    - **Duplicate-vertex invariance:** repeating every vertex changes nothing.
//!    - **Unclipped** (`ClipMode::Unclipped`): `line` accepts the same polylines, and gives each
//!      tile the clipped pass reached the whole line (consecutive duplicates dropped), once, in that
//!      tile's frame; `tile_line` gives exactly that too, there and on every tile of the scanned
//!      span, and nothing elsewhere.
//! 3. Slicing all polylines as one multi-part feature never panics, and a `Mosaic` fed those same
//!    tiles reassembles them:
//!    every tile is accepted (a slicer's own tiles are self-consistent), the reassembled directed-edge
//!    set equals the original input's, insertion order does not matter, and purging every tile empties
//!    it.
//!
//! Coordinates are `i8` (small, so slicing stays fast and the invariants get many cheap
//! iterations); the extent/buffer range freely and the probe tile is a full `i32`, so the
//! single-tile box-overflow handling is stressed too. The oversize/too-many-tiles error paths are
//! covered deterministically by `tests/errors.rs` rather than here.

#![no_main]

use std::collections::{BTreeMap, HashSet};

use arbitrary::Arbitrary;
use geo_types::Coord;
use libfuzzer_sys::fuzz_target;
use map_tile_toolkit::{ClipMode, Mosaic, Slicer, TileId};

/// Cap on tiles scanned by the exhaustive oracle per run (each scanned tile re-walks the polyline),
/// so a tiny extent can't blow up time.
const SCAN_CAP: i64 = 4_096;

#[derive(Arbitrary, Debug)]
struct Input {
    extent: u16,
    buffer: u16,
    /// Polylines of small (`i8`) coordinates, so slicing stays fast and the invariants run cheaply.
    lines: Vec<Vec<(i8, i8)>>,
    /// An arbitrary tile to probe with a single-tile slicer - may be extreme, to stress the tile-box
    /// math.
    probe: (i32, i32),
}

type Runs = Vec<Vec<Coord<i32>>>;

/// Slice a feature (its parts) into all touched tiles, grouped as `tile -> runs` (part by part, in
/// yield order). `None` if the slicer yields an error (overflowing input - a valid outcome).
fn slice_all_map<L: AsRef<[Coord<i32>]>>(
    extent: u32,
    buffer: u16,
    parts: &[L],
) -> Option<BTreeMap<TileId, Runs>> {
    let slicer = Slicer::new(extent, buffer).expect("extent validated");
    let mut map: BTreeMap<TileId, Runs> = BTreeMap::new();
    for part in parts {
        for s in slicer.line(part.as_ref()).ok()? {
            map.entry(s.tile()).or_default().push(s.run().to_vec());
        }
    }
    Some(map)
}

/// Clip one polyline to a single tile, as its runs. Must succeed (callers only use tiles the
/// all-tiles pass produced, or tiles inside the reachable span).
fn slice_one_runs(extent: u32, buffer: u16, tile: TileId, poly: &[Coord<i32>]) -> Runs {
    let slicer = Slicer::new(extent, buffer).expect("extent validated");
    slicer
        .tile_line(tile, poly)
        .expect("an in-range tile")
        .map(|s| s.run().to_vec())
        .collect()
}

fuzz_target!(|input: Input| {
    let extent = u32::from(input.extent);
    let buffer = input.buffer;
    let Ok(slicer) = Slicer::<Coord<i32>>::new(extent, buffer) else {
        return; // extent 0 is rejected - nothing to test
    };

    // Bound the input size so each run stays fast.
    let polylines: Vec<Vec<Coord<i32>>> = input
        .lines
        .iter()
        .take(8)
        .map(|pts| {
            pts.iter()
                .take(64)
                .map(|&(x, y)| Coord {
                    x: i32::from(x),
                    y: i32::from(y),
                })
                .collect()
        })
        .collect();
    let probe = TileId::new(input.probe.0, input.probe.1);

    for poly in &polylines {
        // (1) A single-tile clip on an arbitrary tile must never panic (Ok or Err both fine).
        if let Ok(one) = slicer.tile_line(probe, poly) {
            one.for_each(drop);
        }

        // The all-tiles pass must never panic; Err is a valid outcome for overflowing input.
        let Some(all) = slice_all_map(extent, buffer, &[poly]) else {
            let whole = slicer.clone().with_clip_mode(ClipMode::Unclipped);
            assert!(
                whole.line(poly).is_err(),
                "unclipped accepted a polyline clipped rejects"
            );
            continue;
        };

        // (2) Every all-tiles result round-trips through single-tile slicing (which must succeed).
        for (&tile, runs) in &all {
            let one = slice_one_runs(extent, buffer, tile, poly);
            assert_eq!(
                &one, runs,
                "single-tile disagrees with all-tiles at {tile:?}"
            );
        }

        // (3) Exhaustive per-tile scan of the reachable span reproduces the all-tiles result, when
        // small enough.
        if let Some((lo, hi)) = reachable_span(poly, extent, buffer) {
            let area = i64::from(hi.x - lo.x + 1) * i64::from(hi.y - lo.y + 1);
            if area <= SCAN_CAP {
                let mut scanned = BTreeMap::new();
                for y in lo.y..=hi.y {
                    for x in lo.x..=hi.x {
                        let tile = TileId::new(x, y);
                        let runs = slice_one_runs(extent, buffer, tile, poly);
                        if !runs.is_empty() {
                            scanned.insert(tile, runs);
                        }
                    }
                }
                assert_eq!(all, scanned, "all-tiles and full per-tile scan disagree");
            }
        }

        // (4) Duplicating every vertex must not change the result.
        let duped: Vec<Coord<i32>> = poly.iter().flat_map(|&c| [c, c]).collect();
        let all_duped = slice_all_map(extent, buffer, &[&duped])
            .expect("duplicating vertices cannot make a valid polyline invalid");
        assert_eq!(
            all, all_duped,
            "duplicating every vertex changed the result"
        );

        // (5) Unclipped, every reached tile gets the whole line, once, from both calls.
        check_unclipped(&slicer, poly, &all);
    }

    // (5) Slicing all polylines as one multi-part feature never panics.
    let Some(map) = slice_all_map(extent, buffer, &polylines) else {
        return; // overflowing - nothing more to check
    };

    // (6) `Mosaic` reassembles the slicer's own tiles: self-consistent tiles never conflict, every
    // touched tile registers, and the reassembled global edges equal the original input's. Insertion
    // order must not matter (forward vs reversed give the same connectivity), and purging every tile
    // empties it.
    let build = |reversed: bool| -> Mosaic<Coord<i32>> {
        let mut entries: Vec<(&TileId, &Runs)> = map.iter().collect();
        if reversed {
            entries.reverse();
        }
        let mut mosaic = Mosaic::new(extent).expect("extent validated");
        for (tile, runs) in entries {
            mosaic
                .add(*tile, runs.as_slice())
                .expect("a slicer's own tiles are self-consistent");
        }
        mosaic
    };
    let forward = build(false);
    let reversed = build(true);
    assert_eq!(forward.len(), map.len(), "every touched tile is added");
    let fwd: Runs = forward.iter_features().collect();
    let rev: Runs = reversed.iter_features().collect();
    assert_eq!(
        edge_set(&fwd),
        edge_set(&polylines),
        "mosaic must recover the original global edges"
    );
    assert_eq!(
        edge_set(&fwd),
        edge_set(&rev),
        "mosaic connectivity must not depend on insertion order"
    );

    let mut mosaic = forward;
    for tile in map.keys() {
        assert!(mosaic.purge(*tile), "every added tile purges");
    }
    assert!(mosaic.is_empty(), "purging every tile empties the mosaic");
});

/// Directed-edge set of a run list, skipping zero-length edges - the connectivity a `Mosaic` must
/// preserve regardless of tile insertion order.
fn edge_set(runs: &[Vec<Coord<i32>>]) -> HashSet<(Coord<i32>, Coord<i32>)> {
    let mut set = HashSet::new();
    for run in runs {
        for w in run.windows(2) {
            if w[0] != w[1] {
                set.insert((w[0], w[1]));
            }
        }
    }
    set
}

/// Tile span any piece of `poly` can reach: every vertex's tile grown by the buffer (in tiles) plus
/// one tile of slack. `None` if `poly` is empty. A superset of what the all-tiles pass can return, so
/// scanning it must reproduce that result. Coordinates are `i8`, so the `i64` math cannot overflow.
fn reachable_span(poly: &[Coord<i32>], extent: u32, buffer: u16) -> Option<(TileId, TileId)> {
    let d = i64::from(extent);
    let b = i64::from(buffer);
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for c in poly {
        min_x = min_x.min(i64::from(c.x));
        min_y = min_y.min(i64::from(c.y));
        max_x = max_x.max(i64::from(c.x));
        max_y = max_y.max(i64::from(c.y));
    }
    if poly.is_empty() {
        return None;
    }
    let tile = |v: i64| i32::try_from(v.div_euclid(d)).expect("i8 coords keep tiles in i32");
    Some((
        TileId::new(tile(min_x - b) - 1, tile(min_y - b) - 1),
        TileId::new(tile(max_x + b) + 1, tile(max_y + b) + 1),
    ))
}

/// Unclipped: `line` gives each tile of `all` (the clipped result) the whole line, once, in the
/// tile's frame, and `tile_line` gives exactly that on those tiles and on the scanned span, and
/// nothing (or an error, for a tile out of range) elsewhere.
fn check_unclipped(slicer: &Slicer<Coord<i32>>, poly: &[Coord<i32>], all: &BTreeMap<TileId, Runs>) {
    let whole = slicer.clone().with_clip_mode(ClipMode::Unclipped);
    let (extent, buffer) = (slicer.extent(), slicer.buffer());
    let mut line = poly.to_vec();
    line.dedup();
    let local = |tile: TileId| -> Vec<Coord<i32>> {
        let origin = tile.origin(extent).expect("a reached tile's origin fits");
        line.iter().map(|&c| c - origin).collect()
    };

    let mut tiles = Vec::new();
    for s in whole.line(poly).expect("clipped accepted it") {
        assert_eq!(s.run().to_vec(), local(s.tile()), "not the whole line");
        assert_eq!(s.run().global(), poly, "not lent from the input");
        tiles.push(s.tile());
    }
    let yielded = tiles.len();
    tiles.sort_unstable();
    tiles.dedup();
    assert_eq!(yielded, tiles.len(), "unclipped yielded a tile twice");
    assert!(
        tiles.iter().eq(all.keys()),
        "unclipped reaches other tiles than clipped"
    );

    let probe = |tile: TileId| match whole.tile_line(tile, poly) {
        Ok(slices) => {
            let runs: Runs = slices.map(|s| s.run().to_vec()).collect();
            let expected = if all.contains_key(&tile) {
                vec![local(tile)]
            } else {
                Vec::new()
            };
            assert_eq!(runs, expected, "unclipped tile_line disagrees at {tile:?}");
        }
        Err(_) => assert!(!all.contains_key(&tile), "{tile:?} reached, yet rejected"),
    };
    all.keys().copied().for_each(probe);
    if let Some((lo, hi)) = reachable_span(poly, extent, buffer) {
        if i64::from(hi.x - lo.x + 1) * i64::from(hi.y - lo.y + 1) <= SCAN_CAP {
            (lo.y..=hi.y)
                .flat_map(|y| (lo.x..=hi.x).map(move |x| TileId::new(x, y)))
                .for_each(probe);
        }
    }
}

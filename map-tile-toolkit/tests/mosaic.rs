//! [`Mosaic`] reassembles sliced tiles back into whole features.
//!
//! One data-driven test loads **every** fixture (sorted), slices them all into one set of tiles, then
//! for **every permutation** of the tile-insertion order builds a fresh mosaic, checks every `add`
//! succeeds (a slicer's own tiles are self-consistent, so none ever conflict), and checks the
//! reassembled geometry equals the combined input's - proving order-independence over all fixtures at
//! once. It runs at **both** buffer sizes: flush tiles (buffer 0) and overlapping tiles (buffer 5).
//! A buffer only duplicates near-edge segments into more tiles, so rebasing (`local + tile·extent`)
//! must collapse those duplicates back onto the same global edges - either way the mosaic recovers
//! the exact same original geometry. Up to seven tiles, every insertion order is tried; a larger
//! fixture set is checked under a deterministic spread of orders (identity, reverse, every rotation).
//!
//! Reassembly is compared by **directed-edge set**: the mosaic re-chains the geometry by connectivity,
//! so a self-touching path or a shared junction may come back split/joined differently than the input
//! features, but the set of edges - the geometry - is identical.

#![allow(clippy::pedantic, reason = "test tool")]

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use geo_types::Coord;
use map_tile_toolkit::{Mosaic, TileError, TileId};

use crate::support::{Cfg, orders};

mod support;

/// Every fixture's polylines, files sorted for a stable order.
fn all_fixture_polylines() -> Vec<Vec<Coord<i32>>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/polylines/fixtures");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("fixtures dir exists")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("geojson"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no fixtures found");
    paths.iter().flat_map(|p| support::load_fixture_geoms(p)).collect()
}

/// A polyline of plain `Coord`s.
fn line(coords: &[(i32, i32)]) -> Vec<Coord<i32>> {
    coords.iter().map(|&(x, y)| Coord { x, y }).collect()
}

/// Directed-edge set of a run list, skipping zero-length edges (slicing drops consecutive dups).
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

#[test]
fn every_permutation_reassembles_all_fixtures() {
    let polylines = all_fixture_polylines();
    for (label, cfg) in [("buffer 0", support::grid()), ("buffer 5", support::grid_buffered())] {
        validate_mosaic(label, &cfg, &polylines);
    }
}

/// Slice all fixtures into per-tile (local-frame) runs - exactly what a caller feeds back - then check
/// that **every** tile-insertion order reassembles the exact same features (order-independence) and
/// that those features match the input by directed-edge set.
///
/// Correctness is asserted directly (`reassembled == input`), not against a golden file: reassembly is
/// a pure function of the edge set, so all orders (and both buffers) produce identical features, and the
/// edge-set equality is invariant to how `stitch` happens to split/join chains at shared junctions.
fn validate_mosaic(label: &str, cfg: &Cfg, polylines: &[Vec<Coord<i32>>]) {
    let tiles = support::slice_all_runs(cfg, polylines);
    assert!(!tiles.is_empty(), "{label}: fixtures produced no tiles");

    let mut canonical: Option<Vec<Vec<Coord<i32>>>> = None;
    for order in orders(tiles.len(), 7) {
        let mut mosaic = Mosaic::new(cfg.extent).expect("valid config");
        for &i in &order {
            let (tile, runs) = &tiles[i];
            mosaic
                .add(*tile, runs.as_slice())
                .expect("a slicer's own tiles are self-consistent and never conflict");
        }
        assert_eq!(mosaic.len(), tiles.len(), "{label}: every tile must register");

        // The mosaic reassembles in the global frame - the input's own space at any buffer.
        let features: Vec<Vec<Coord<i32>>> = mosaic.iter_features().collect();
        match &canonical {
            None => canonical = Some(features),
            Some(first) => assert_eq!(
                &features, first,
                "{label}: insertion order {order:?} changed the reassembly"
            ),
        }
    }

    // Correctness against the input: the reassembled geometry is the combined input geometry.
    let features = canonical.expect("at least one permutation");
    assert_eq!(
        edge_set(&features),
        edge_set(polylines),
        "{label}: reassembly did not reconstruct the combined input geometry"
    );
}

// --- Behaviors this valid-data file doesn't cover: payload conflicts live in payload.rs, membership
// conflicts (inconsistent tiles) in mosaic_bad.rs. ---

#[test]
fn bad_config_and_far_tiles_are_errors_and_an_empty_mosaic_is_empty() {
    for extent in [0, u32::MAX] {
        assert_eq!(Mosaic::<Coord<i32>>::new(extent).err(), Some(TileError::InvalidExtent));
    }
    let empty = Mosaic::<Coord<i32>>::new(25).expect("valid config");
    assert_eq!(empty.iter_features().count(), 0);

    // A tile whose global frame leaves `i32` is an error, not a panic, and changes nothing.
    let mut mosaic = Mosaic::new(4096).expect("valid config");
    let run = vec![Coord { x: 0, y: 0 }, Coord { x: 1, y: 0 }];
    let bad = mosaic.add(TileId::new(i32::MAX, 0), &[run]);
    assert_eq!(bad, Err(TileError::Overflow));
    assert!(mosaic.is_empty(), "an overflowing add changes nothing");
}

#[test]
fn purge_and_clear_manage_tiles() {
    let mut mosaic = Mosaic::new(25).expect("valid config");
    assert!(mosaic.is_empty());

    // Two disjoint segments in different tiles.
    mosaic
        .add(TileId::new(0, 0), &[line(&[(1, 1), (10, 10)])])
        .expect("tile 0");
    mosaic
        .add(TileId::new(5, 5), &[line(&[(3, 3), (12, 12)])])
        .expect("tile 5");
    assert_eq!(mosaic.len(), 2);
    assert!(mosaic.contains(TileId::new(0, 0)));
    assert!(!mosaic.contains(TileId::new(9, 9)));

    // Purge one tile: it and only it disappears.
    assert!(mosaic.purge(TileId::new(0, 0)));
    assert!(!mosaic.contains(TileId::new(0, 0)));
    assert!(mosaic.contains(TileId::new(5, 5)));
    assert_eq!(mosaic.len(), 1);
    assert!(!mosaic.purge(TileId::new(0, 0)), "purging an absent tile is a no-op");

    // Clear drops everything.
    mosaic.clear();
    assert!(mosaic.is_empty());
    assert_eq!(mosaic.len(), 0);
}

#[test]
fn purge_keeps_edges_other_tiles_still_hold() {
    // Both tiles carry the same global border segment (20,5)->(30,5) - tile (1,0)'s local run rebases
    // onto it. Purging one tile must leave the shared edge, since the other still holds it.
    let mut mosaic = Mosaic::new(25).expect("valid config");
    mosaic
        .add(TileId::new(0, 0), &[line(&[(20, 5), (30, 5)])])
        .expect("tile 0");
    mosaic
        .add(TileId::new(1, 0), &[line(&[(-5, 5), (5, 5)])])
        .expect("tile 1");

    assert!(mosaic.purge(TileId::new(1, 0)));
    let feats: Vec<Vec<Coord<i32>>> = mosaic.iter_features().collect();
    assert_eq!(
        feats,
        vec![line(&[(20, 5), (30, 5)])],
        "the shared edge survives while tile (0,0) still holds it"
    );

    // Purging the last holder drops the edge and empties the mosaic.
    assert!(mosaic.purge(TileId::new(0, 0)));
    assert!(mosaic.is_empty());
}

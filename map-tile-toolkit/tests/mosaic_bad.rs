//! Bad-data tests for [`Mosaic`]: hand-crafted **inconsistent** tile sets that a real slicer could
//! never produce, which the mosaic must reject with [`TileError::Conflict`].
//!
//! Each `tests/bad-fixtures/*.geojson` is a `FeatureCollection` whose features carry a
//! `"role": "tile x/y"` property naming the tile that (supposedly) contributed them, in global
//! coordinates (extent [`EXTENT`]); several features may share a tile. Every fixture is loaded,
//! grouped by tile, and - for **every** insertion order - fed to a fresh mosaic; some `add` must
//! return a `Conflict` (order-independent detection), and the failing `add` must leave the mosaic
//! unchanged (atomicity).
//!
//! The inconsistencies exercised (see the fixtures): a feature spanning into a neighbor that never
//! corroborates it, two tiles disagreeing on a shared edge's far endpoint, on both endpoints, on its
//! direction, a middle tile missing the segments that pass through its core, and - from a buffered
//! slice - a tile whose boundary-crossing edge is correct but a *further* overlap vertex is moved
//! (the moved vertex lands in some tile's core, so the core-completeness check rejects it without the
//! mosaic ever needing the buffer).

#![allow(clippy::pedantic, clippy::panic, reason = "test tool")]

use std::collections::BTreeMap;
use std::path::Path;

use geo_types::Coord;
use geojson::JsonValue;
use map_tile_toolkit::{Mosaic, TileError, TileId};

use crate::support::{EXTENT, permutations};

mod support;

mod files {
    use test_each_file::test_each_path;

    test_each_path! { for ["geojson"] in "./map-tile-toolkit/tests/polylines/bad-fixtures" => super::one_bad_fixture }
}

/// Parse `"tile x/y"` into a [`TileId`].
fn parse_tile(role: &str) -> TileId {
    let coords = role.strip_prefix("tile ").expect("role is `tile x/y`");
    let (x, y) = coords.split_once('/').expect("role is `tile x/y`");
    TileId::new(
        x.trim().parse().expect("integer tile x"),
        y.trim().parse().expect("integer tile y"),
    )
}

/// Load a bad fixture into `(tile, local-frame runs)` groups - the exact shape a caller feeds
/// [`Mosaic::add`]. Each feature's `role` gives its tile; its global coordinates are localized
/// (`global - tile·extent`) so the mosaic rebases them straight back.
fn load(path: &Path) -> Vec<(TileId, Vec<Vec<Coord<i32>>>)> {
    let mut groups: BTreeMap<TileId, Vec<Vec<Coord<i32>>>> = BTreeMap::new();
    for feature in support::load_fixture(path) {
        let tile = parse_tile(
            feature
                .properties
                .get("role")
                .and_then(JsonValue::as_str)
                .expect("each feature needs a `role` property"),
        );
        let origin = tile.origin(EXTENT).expect("fixture tile in range");
        let run = feature.line.iter().map(|&c| c - origin).collect();
        groups.entry(tile).or_default().push(run);
    }
    groups.into_iter().collect()
}

/// Assert that **every** insertion order of a bad fixture's tiles is rejected with a `Conflict`, and
/// each rejected `add` leaves the mosaic unchanged.
fn one_bad_fixture([path]: [&Path; 1]) {
    let name = path.file_stem().and_then(|s| s.to_str()).expect("stem");
    let tiles = load(path);
    assert!(
        tiles.len() >= 2,
        "{name}: a conflict needs at least two tiles"
    );

    for order in permutations(tiles.len()) {
        let mut mosaic = Mosaic::new(EXTENT).expect("valid config");
        let mut rejected = false;
        for &i in &order {
            let (tile, runs) = &tiles[i];
            let before = mosaic.len();
            match mosaic.add(*tile, runs.as_slice()) {
                Ok(()) => {}
                Err(TileError::Conflict(named)) => {
                    assert!(!named.is_empty(), "{name}: a conflict must name tiles");
                    assert_eq!(
                        mosaic.len(),
                        before,
                        "{name}: a rejected add must leave the mosaic unchanged"
                    );
                    rejected = true;
                }
                Err(other) => panic!("{name}: unexpected error {other:?}"),
            }
        }
        assert!(
            rejected,
            "{name}: insertion order {order:?} accepted inconsistent tiles"
        );
    }
}

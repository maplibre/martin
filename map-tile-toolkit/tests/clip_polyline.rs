//! Insta GeoJSON snapshots for the integer polyline slicer.
//!
//! Each fixture in `tests/polylines/fixtures/*.geojson` is a `FeatureCollection` with one or more `LineString`
//! features (whole-number coordinates in valid lon/lat range so the fixtures and snapshots render on
//! a map); each feature is an independent polyline. Every fixture is sliced two ways and the two
//! must be **byte identical**:
//!
//! 1. `slice_all_tiles` - the whole geometry into every tile it touches, in one pass.
//! 2. For each tile that (1) produced, `slice_tile` re-clips that single tile.
//!
//! The result is snapshotted as a `FeatureCollection`: the original polyline and one feature per
//! per-tile piece, sorted by `role`. Regenerate with `just bless`.
//!
//! Every fixture is snapshotted at two buffer sizes, each into its own directory:
//! - `snapshots/` - buffer 0 (tile boxes flush with the grid);
//! - `snapshots-5/` - buffer 5 (each tile box grown 5 units per side, so near-edge vertices and
//!   crossing segments also land in the neighboring tiles).

#![allow(clippy::pedantic, reason = "test/inspection tool")]

use std::collections::BTreeMap;
use std::path::Path;

use geo_types::Coord;
use insta::assert_binary_snapshot;
use map_tile_toolkit::TileId;

mod support;
use crate::support::{EXTENT, feature, feature_line, load_fixture_geoms};

/// Buffer sizes each fixture is snapshotted at, paired with the directory to write into. Buffer 0
/// keeps the tile boxes flush with the grid; buffer 5 (a fifth of a tile) grows each box outward so
/// near-edge vertices and crossing segments also land in the neighboring tiles.
fn slicers() -> [(support::Cfg, &'static str); 2] {
    [
        (support::grid(), "polylines/snapshots"),
        (support::grid_buffered(), "polylines/snapshots-5"),
    ]
}

mod files {
    use test_each_file::test_each_path;

    // Generate one test per input fixture.
    test_each_path! { for ["geojson"] in "./map-tile-toolkit/tests/polylines/fixtures" => super::slice_one_fixture }
}

/// Inclusive tile-coordinate bounds covering every vertex of `polylines`, padded by one tile so the
/// per-tile scan also checks the empty tiles just outside the geometry.
fn padded_tile_span(polylines: &[Vec<Coord<i32>>]) -> (TileId, TileId) {
    let mut lo = TileId::new(i32::MAX, i32::MAX);
    let mut hi = TileId::new(i32::MIN, i32::MIN);
    for line in polylines {
        for &c in line {
            let (tx, ty) = (c.x.div_euclid(EXTENT as i32), c.y.div_euclid(EXTENT as i32));
            lo = TileId::new(lo.x.min(tx), lo.y.min(ty));
            hi = TileId::new(hi.x.max(tx), hi.y.max(ty));
        }
    }
    (
        TileId::new(lo.x - 1, lo.y - 1),
        TileId::new(hi.x + 1, hi.y + 1),
    )
}

/// A copy of `polylines` with every vertex repeated once - consecutive duplicates the slicers must
/// transparently drop, so clipping the copy yields the same result as the original.
fn duplicate_vertices(polylines: &[Vec<Coord<i32>>]) -> Vec<Vec<Coord<i32>>> {
    polylines
        .iter()
        .map(|line| line.iter().flat_map(|&c| [c, c]).collect())
        .collect()
}

/// Undo tile-local normalization: add `tile`'s origin (`tile · extent`) back to every vertex of each
/// run, so the pieces are in the input's global coordinate space for rendering. Validation happens in
/// local coords; only the snapshot files are written globally (so they still line up on the map grid).
fn globalize(tile: TileId, runs: &[Vec<Coord<i32>>], extent: i32) -> Vec<Vec<Coord<i32>>> {
    let origin = tile.origin(extent.cast_unsigned()).expect("tile in range");
    runs.iter()
        .map(|run| run.iter().map(|&c| c + origin).collect())
        .collect()
}

/// Build the snapshot features: the input polylines (one gray feature each) and one feature
/// per per-tile **run** - each a plain `LineString`, never a `MultiLineString`, so distinct
/// features/runs in a tile stay distinct (colored by tile parity so neighbors contrast, tagged with
/// the tile).
fn build_features(
    input: &[Vec<Coord<i32>>],
    tiles: &BTreeMap<TileId, Vec<Vec<Coord<i32>>>>,
) -> Vec<geojson::Feature> {
    let mut features: Vec<_> = input
        .iter()
        .map(|line| support::input_feature(line))
        .collect();
    for (tile, runs) in tiles {
        for run in runs {
            features.push(feature_line(run, &format!("tile {}/{}", tile.x, tile.y)));
        }
    }
    features
}

fn slice_one_fixture([path]: [&Path; 1]) {
    let stem = path.file_stem().and_then(|s| s.to_str()).expect("stem");
    let polylines = load_fixture_geoms(path);
    for (slicer, snapshot_dir) in &slicers() {
        slice_at_buffer(slicer, stem, &polylines, snapshot_dir);
    }
}

#[test]
#[ignore = "manually save big geometry"]
fn save_big_geometry() {
    let bytes = support::feature_collection_bytes(vec![feature(
        &support::to_f64(&support::big_polyline()),
        vec![],
    )]);
    std::fs::write("tests/polylines/fixtures/big-geometry.geojson", bytes).expect("writes");
}

/// The shared `BIG_CONFIGS` slicers must slice the big polyline into the documented number of
/// tiles: `single` -> one, `few` -> a 2×2 grid of four, `multi` -> many. Guards the extent choices
/// the benchmarks and the `profile` example rely on.
#[test]
fn big_config_tile_counts() {
    let geom = support::polylines_of(&support::big_polyline());
    for (name, slicer) in support::big_configs() {
        let n = support::slice_all_runs(&slicer, &geom).len();
        match name {
            "single" => assert_eq!(n, 1, "`single` should keep the whole polyline in one tile"),
            "few" => assert_eq!(n, 4, "`few` should produce a 2×2 grid of tiles"),
            "multi" => assert!(n > 100, "`multi` should produce many tiles, got {n}"),
            other => panic!("unexpected config {other}"),
        }
    }
}

/// A z14-scale diagonal - one segment across 16384 × 16384 tiles of extent 4096 - slices (routing
/// charges the tiles it touches, not its 2^28-tile bounding box), and every sampled output tile and
/// each of its neighbours agrees with a per-tile [`Slicer::tile_line`](map_tile_toolkit::Slicer::tile_line) clip.
#[test]
fn long_diagonal_slices_at_z14() {
    const N: i32 = 1 << 14;
    let cfg = support::slicer(support::DIAGONAL_EXTENT as u32, 64);
    let line = vec![support::long_diagonal(N)];
    let all: BTreeMap<TileId, Vec<Vec<Coord<i32>>>> =
        support::slice_all_runs(&cfg, &line).into_iter().collect();
    // Each tile row holds the one or two tiles the line crosses, plus buffer neighbours.
    assert!(
        all.len() > 2 * N as usize && all.len() < 4 * N as usize,
        "{}",
        all.len()
    );
    assert!(all.contains_key(&TileId::new(0, 0)) && all.contains_key(&TileId::new(N - 1, N - 1)));
    for t in all.keys().step_by(97) {
        for (dx, dy) in (-1..=1).flat_map(|dx| (-1..=1).map(move |dy| (dx, dy))) {
            let tile = TileId::new(t.x + dx, t.y + dy);
            let expected = support::slice_tile_runs(&cfg, &line, tile);
            assert_eq!(
                all.get(&tile).cloned().unwrap_or_default(),
                expected,
                "{tile:?}"
            );
        }
    }
}

/// Run every cross-check for one fixture at one buffer size, then snapshot the result into
/// `snapshot_dir`.
fn slice_at_buffer(
    slicer: &support::Cfg,
    stem: &str,
    geom: &[Vec<Coord<i32>>],
    snapshot_dir: &str,
) {
    // (1) Slice the whole geometry into every tile it touches.
    let all: BTreeMap<TileId, Vec<Vec<Coord<i32>>>> =
        support::slice_all_runs(slicer, geom).into_iter().collect();

    // (2) Independently, clip one tile at a time across the whole tile span the geometry could
    // reach (padded by one tile). Collecting every non-empty result must reproduce `all` exactly -
    // this checks the batch found no wrong pieces and missed no tile (including tiles a segment
    // only crosses, which both paths must include).
    let (lo, hi) = padded_tile_span(geom);
    let mut one = BTreeMap::new();
    for y in lo.y..=hi.y {
        for x in lo.x..=hi.x {
            let tile = TileId::new(x, y);
            let piece = support::slice_tile_runs(slicer, geom, tile);
            if !piece.is_empty() {
                one.insert(tile, piece);
            }
        }
    }
    assert_eq!(
        all, one,
        "batch and per-tile slicing disagree for {stem} (buffer {})",
        slicer.buffer
    );

    // (3) Duplicating every vertex must not change either slicer's output (consecutive dups are
    // dropped), so both paths on the duplicated input still match the original result.
    let duped = duplicate_vertices(geom);
    let all_duped: BTreeMap<TileId, Vec<Vec<Coord<i32>>>> = support::slice_all_runs(slicer, &duped)
        .into_iter()
        .collect();
    assert_eq!(
        all_duped, all,
        "duplicating every vertex changed the batch result for {stem} (buffer {})",
        slicer.buffer
    );
    for (&tile, piece) in &all {
        let piece_dup = support::slice_tile_runs(slicer, &duped, tile);
        assert!(
            !piece_dup.is_empty(),
            "tile {tile:?} vanished after vertex duplication for {stem}"
        );
        assert_eq!(
            &piece_dup, piece,
            "duplicated-vertex per-tile differs at {tile:?} for {stem}"
        );
    }

    // The two snapshots must be byte identical; snapshot the (shared) result. Pieces come back in
    // tile-local coordinates, so convert them back to the global space before rendering.
    let extent = slicer.extent as i32;
    let global =
        |m: &BTreeMap<TileId, Vec<Vec<Coord<i32>>>>| -> BTreeMap<TileId, Vec<Vec<Coord<i32>>>> {
            m.iter()
                .map(|(&t, runs)| (t, globalize(t, runs, extent)))
                .collect()
        };
    let all_bytes = support::feature_collection_bytes(build_features(geom, &global(&all)));
    let one_bytes = support::feature_collection_bytes(build_features(geom, &global(&one)));
    assert_eq!(
        all_bytes, one_bytes,
        "batch and per-tile snapshots differ for {stem} (buffer {})",
        slicer.buffer
    );

    insta::with_settings!({
        snapshot_path => snapshot_dir,
        prepend_module_to_snapshot => false,
    }, {
        let name = if slicer.buffer > 0 {
            format!("{stem}-{}.geojson", slicer.buffer)
        } else {
            format!("{stem}.geojson")
        };
        assert_binary_snapshot!(&name, all_bytes);
    });
}

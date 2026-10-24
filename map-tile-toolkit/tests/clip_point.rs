//! Insta GeoJSON snapshots for point slicing, mirroring `clip_polyline.rs` and `clip_polygon.rs`.
//!
//! Each fixture in `tests/points/fixtures/*.geojson` is a `FeatureCollection` of `Point` or
//! `MultiPoint` features on the same 25-unit grid as the polyline and polygon fixtures; a multipoint
//! feature is sliced one point at a time. Every fixture is sliced three ways, which must agree:
//!
//! 1. [`Slicer::point`] - every feature into every tile it reaches, in one pass.
//! 2. [`Slicer::tile_point`] - one tile at a time, across the tile span the points could reach
//!    (padded by one tile).
//! 3. An independent oracle: a tile holds a point iff its closed buffered box
//!    `[t·extent - buffer, (t+1)·extent + buffer]` holds it on both axes.
//!
//! The result is snapshotted as a `FeatureCollection` of two kinds of `MultiPoint` features, sorted by
//! `role`: one with every input point (yellow), and, per tile, one with that tile's copies in global
//! coordinates, tagged with the tile and the `indices` of the input points they copy (positions in the input
//! `MultiPoint`). Regenerate with
//! `just bless`.
//!
//! Every fixture is snapshotted at two buffer sizes: `points/snapshots/` (buffer 0, each point in
//! one tile unless on its edge) and `points/snapshots-5/` (buffer 5, near-edge points also in the neighbors).

#![allow(clippy::pedantic, reason = "test/inspection tool")]

use std::collections::BTreeMap;
use std::path::Path;

use geo_types::Coord;
use insta::assert_binary_snapshot;
use map_tile_toolkit::{Slicer, TileId};

mod support;

use crate::support::EXTENT;

/// Buffer sizes each fixture is snapshotted at, paired with the directory to write into.
fn buffers() -> [(u16, &'static str); 2] {
    [(0, "points/snapshots"), (5, "points/snapshots-5")]
}

mod files {
    use test_each_file::test_each_path;

    // One test per input fixture.
    test_each_path! { for ["geojson"] in "./map-tile-toolkit/tests/points/fixtures" => super::snapshot_point_fixture }
}

/// Each tile's points as `(feature, index, tile-local point)`, ordered by feature, then index. A
/// multipoint feature is sliced one point at a time, so a point's index is its place in the feature.
type Tiles = BTreeMap<TileId, Vec<(usize, u32, Coord<i32>)>>;

/// Slice every point of every feature with [`Slicer::point`], grouped by tile.
fn slice_all(slicer: &Slicer<Coord<i32>>, features: &[Vec<Coord<i32>>]) -> Tiles {
    let mut out = Tiles::new();
    for (f, points) in features.iter().enumerate() {
        for (index, &point) in (0_u32..).zip(points) {
            for p in slicer.point(point).expect("point") {
                out.entry(p.tile()).or_default().push((f, index, p.point()));
            }
        }
    }
    // Slice order is unspecified; compare each tile's points in input order.
    for points in out.values_mut() {
        points.sort_by_key(|&(f, index, _)| (f, index));
    }
    out
}

/// Slice every point of every feature into one `tile` with [`Slicer::tile_point`].
fn slice_tile(
    slicer: &Slicer<Coord<i32>>,
    features: &[Vec<Coord<i32>>],
    tile: TileId,
) -> Vec<(usize, u32, Coord<i32>)> {
    let mut out = Vec::new();
    for (f, points) in features.iter().enumerate() {
        for (index, &point) in (0_u32..).zip(points) {
            if let Some(p) = slicer.tile_point(tile, point).expect("tile") {
                out.push((f, index, p));
            }
        }
    }
    out
}

/// The tiles in `lo..=hi` whose closed buffered box holds each point, computed directly from the
/// box bounds - independent of the slicer's per-point tile arithmetic.
fn oracle(features: &[Vec<Coord<i32>>], buffer: u16, lo: TileId, hi: TileId) -> Tiles {
    let (e, b) = (EXTENT as i32, i32::from(buffer));
    let holds = |t: i32, v: i32| t * e - b <= v && v <= (t + 1) * e + b;
    let mut out = Tiles::new();
    for y in lo.y..=hi.y {
        for x in lo.x..=hi.x {
            let tile = TileId::new(x, y);
            let origin = Coord { x: x * e, y: y * e };
            for (f, points) in features.iter().enumerate() {
                for (index, &p) in points.iter().enumerate() {
                    if holds(x, p.x) && holds(y, p.y) {
                        let index = u32::try_from(index).expect("small fixture");
                        out.entry(tile).or_default().push((f, index, p - origin));
                    }
                }
            }
        }
    }
    out
}

fn snapshot_point_fixture([path]: [&Path; 1]) {
    let stem = path.file_stem().and_then(|s| s.to_str()).expect("stem");
    let features = support::load_point_features(path);
    for (buffer, dir) in buffers() {
        snapshot_at_buffer(stem, &features, buffer, dir);
    }
}

fn snapshot_at_buffer(stem: &str, features: &[Vec<Coord<i32>>], buffer: u16, dir: &str) {
    let slicer = Slicer::<Coord<i32>>::new(EXTENT, buffer).expect("valid config");

    // (1) Every feature into every tile it reaches, in one pass.
    let all = slice_all(&slicer, features);

    // (2) One tile at a time across the padded span: every non-empty tile must reproduce `all`,
    // so the one-pass slicer found no wrong point and missed no tile.
    let (lo, hi) = support::padded_tile_span(features);
    let mut one = Tiles::new();
    for y in lo.y..=hi.y {
        for x in lo.x..=hi.x {
            let tile = TileId::new(x, y);
            let points = slice_tile(&slicer, features, tile);
            if !points.is_empty() {
                one.insert(tile, points);
            }
        }
    }
    assert_eq!(
        all, one,
        "Slicer::point and Slicer::tile_point differ for {stem} (buffer {buffer})"
    );

    // (3) Both must match the closed-box rule computed directly.
    assert_eq!(
        all,
        oracle(features, buffer, lo, hi),
        "slicing differs from the closed buffered box rule for {stem} (buffer {buffer})"
    );

    // Build the snapshot: one `MultiPoint` with every input point, then one per tile with that tile's
    // points moved back to the global frame; serialization sorts them by role.
    let inputs: Vec<_> = features.iter().flatten().copied().collect();
    // Where each feature's points start in `inputs`, so a copy names the input point it is of.
    let starts: Vec<u32> = features
        .iter()
        .scan(0, |at, f| {
            let start = *at;
            *at += u32::try_from(f.len()).expect("small fixture");
            Some(start)
        })
        .collect();
    let mut out = vec![support::input_points(&inputs)];
    for (&tile, points) in &all {
        let origin = tile.origin(EXTENT).expect("tile in range");
        let global: Vec<_> = points.iter().map(|&(_, _, p)| p + origin).collect();
        let indices: Vec<_> = points.iter().map(|&(f, index, _)| starts[f] + index).collect();
        out.push(support::tile_points(&global, tile, &indices));
    }
    let bytes = support::feature_collection_bytes(out);

    insta::with_settings!({
        snapshot_path => dir,
        prepend_module_to_snapshot => false,
    }, {
        let name = if buffer > 0 {
            format!("{stem}-{buffer}.geojson")
        } else {
            format!("{stem}.geojson")
        };
        assert_binary_snapshot!(&name, bytes);
    });
}

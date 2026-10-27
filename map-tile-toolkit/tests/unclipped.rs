//! Insta GeoJSON snapshots for [`ClipMode::Unclipped`] slicing, over every polyline and polygon
//! fixture at buffers 0 and 5.
//!
//! An unclipped slicer gives every tile a geometry reaches the **whole** geometry, once, in that
//! tile's local frame. So for every fixture:
//!
//! 1. [`Slicer::line`] / [`Slicer::polygon`] yield each tile at most once per geometry, and exactly
//!    the tiles the clipped slicer gives anything (a polygon's fill-run tiles included, as pieces:
//!    no run is yielded);
//! 2. every slice is the whole input in the tile's frame - a line without its consecutive duplicate
//!    vertices, a polygon as each of its non-degenerate rings, distinct and closed - and its
//!    [`Local::origin`] / [`Local::global`] describe that frame;
//! 3. [`Slicer::tile_line`] / [`Slicer::tile_polygon`], asked for every tile of the padded span,
//!    give exactly the same, and nothing elsewhere.
//!
//! What a tile holds is then the input, so the snapshot shows which tiles hold it: the input (yellow)
//! and every reached tile's buffered box, tagged with the tile and how many geometries it holds.
//! `snapshots-unclipped/` is buffer 0; buffer 5 runs every check above but is not snapshotted (the
//! buffered boxes would only grow the same picture). Regenerate with `just bless`.
//!
//! [`Local::origin`]: map_tile_toolkit::Local::origin
//! [`Local::global`]: map_tile_toolkit::Local::global

#![allow(clippy::pedantic, clippy::panic, reason = "test tool")]

use std::collections::BTreeMap;
use std::path::Path;

use geo_types::Coord;
use insta::assert_binary_snapshot;
use map_tile_toolkit::{ClipMode, Local, PolygonSlice, Slicer, TileId};
use serde_json::json;

mod support;

use crate::support::{EXTENT, FixturePolygon};

mod lines {
    use test_each_file::test_each_path;

    test_each_path! { for ["geojson"] in "./map-tile-toolkit/tests/polylines/fixtures" => super::lines }
}

mod polygons {
    use test_each_file::test_each_path;

    test_each_path! { for ["geojson"] in "./map-tile-toolkit/tests/polygons/fixtures" => super::polygons }
}

/// An unclipped slicer.
fn unclipped(buffer: u16) -> Slicer {
    Slicer::new(EXTENT, buffer)
        .expect("valid config")
        .with_clip_mode(ClipMode::Unclipped)
}

/// `ring` without consecutive duplicate positions.
fn dedup(ring: &[Coord<i32>]) -> Vec<Coord<i32>> {
    let mut out = ring.to_vec();
    out.dedup();
    out
}

/// A polygon's rings as an unclipped slice lends them: each distinct (no consecutive duplicates, no
/// repeated closing vertex) and then closed, rings of fewer than 3 distinct vertices dropped - and no
/// rings at all if that drops the exterior.
fn whole_rings(polygon: &FixturePolygon) -> Vec<Vec<Coord<i32>>> {
    let mut rings = Vec::new();
    for (i, ring) in support::rings(polygon).into_iter().enumerate() {
        let mut ring = dedup(ring);
        while ring.len() >= 2 && ring.last() == ring.first() {
            ring.pop();
        }
        if ring.len() < 3 {
            if i == 0 {
                return Vec::new();
            }
            continue;
        }
        ring.push(ring[0]);
        rings.push(ring);
    }
    rings
}

/// The local vertices of `ring`, lent for `tile`, after checking that its frame is `tile`'s.
fn local(tile: TileId, ring: Local<'_, Coord<i32>>) -> Vec<Coord<i32>> {
    let origin = tile.origin(EXTENT).expect("tile in range");
    assert_eq!(ring.origin(), origin, "tile {tile:?} origin");
    let local = ring.to_vec();
    let from_global = dedup(&ring.global().iter().map(|&g| g - origin).collect::<Vec<_>>());
    assert_eq!(
        local[..from_global.len()],
        from_global[..],
        "tile {tile:?} global() is not its local vertices moved by its origin"
    );
    local
}

/// `geom` moved into `tile`'s local frame.
fn to_local(tile: TileId, geom: &[Vec<Coord<i32>>]) -> Vec<Vec<Coord<i32>>> {
    let origin = tile.origin(EXTENT).expect("tile in range");
    geom.iter()
        .map(|ring| ring.iter().map(|&c| c - origin).collect())
        .collect()
}

/// Every tile of the span the geometry `rings` could reach, padded by one tile.
fn padded_tiles(rings: &[Vec<Coord<i32>>]) -> impl Iterator<Item = TileId> {
    let (lo, hi) = support::padded_tile_span(rings);
    (lo.y..=hi.y).flat_map(move |y| (lo.x..=hi.x).map(move |x| TileId::new(x, y)))
}

fn lines([path]: [&Path; 1]) {
    let stem = path.file_stem().and_then(|s| s.to_str()).expect("stem");
    let lines = support::load_fixture_geoms(path);
    for buffer in [0, 5] {
        let what = format!("{stem} (buffer {buffer})");
        let slicer = unclipped(buffer);
        let clipped = support::slicer(EXTENT, buffer);
        // How many lines each tile holds.
        let mut tiles = BTreeMap::<TileId, u32>::new();
        for line in &lines {
            let mut reached = Vec::new();
            for slice in slicer.line(line).expect("line") {
                let tile = slice.tile();
                assert_eq!(
                    local(tile, slice.run()),
                    to_local(tile, &[dedup(line)])[0],
                    "{what}: tile {tile:?} is not the whole line"
                );
                assert_eq!(slice.run().global(), &line[..], "{what}: not the whole line");
                reached.push(tile);
            }
            let yielded = reached.len();
            reached.sort_unstable();
            reached.dedup();
            assert_eq!(yielded, reached.len(), "{what}: a tile came twice");
            let expected: Vec<_> = support::slice_all_runs(&clipped, std::slice::from_ref(line))
                .into_iter()
                .map(|(tile, _)| tile)
                .collect();
            assert_eq!(reached, expected, "{what}: not the clipped slicer's tiles");
            for tile in padded_tiles(std::slice::from_ref(line)) {
                let one: Vec<_> = slicer
                    .tile_line(tile, line)
                    .expect("tile_line")
                    .map(|s| local(tile, s.run()))
                    .collect();
                let expected = if reached.binary_search(&tile).is_ok() {
                    to_local(tile, &[dedup(line)])
                } else {
                    Vec::new()
                };
                assert_eq!(one, expected, "{what}: tile_line differs at {tile:?}");
            }
            for tile in reached {
                *tiles.entry(tile).or_default() += 1;
            }
        }
        let inputs = lines.iter().map(|l| support::input_feature(l));
        snapshot("polylines", stem, buffer, inputs.collect(), &tiles);
    }
}

fn polygons([path]: [&Path; 1]) {
    let stem = path.file_stem().and_then(|s| s.to_str()).expect("stem");
    let polygons = support::load_polygon_fixture(path);
    for buffer in [0, 5] {
        let what = format!("{stem} (buffer {buffer})");
        let mut slicer = unclipped(buffer);
        let mut clipped = Slicer::new(EXTENT, buffer).expect("valid config");
        let mut tiles = BTreeMap::<TileId, u32>::new();
        for polygon in &polygons {
            let rings = support::rings(polygon);
            let whole = whole_rings(polygon);
            let mut reached = Vec::new();
            let mut slices = slicer.polygon(&rings).expect("polygon");
            while let Some(slice) = slices.next() {
                let PolygonSlice::Piece(piece) = slice.expect("slice") else {
                    panic!("{what}: an unclipped slicer yielded a fill run");
                };
                let tile = piece.tile();
                let got: Vec<_> = piece.rings().map(|r| local(tile, r)).collect();
                assert_eq!(got, to_local(tile, &whole), "{what}: tile {tile:?} is not whole");
                reached.push(tile);
            }
            let yielded = reached.len();
            reached.sort_unstable();
            reached.dedup();
            assert_eq!(yielded, reached.len(), "{what}: a tile came twice");
            let expected: Vec<_> = support::slice_polygons(&mut clipped, std::slice::from_ref(polygon))
                .expect("clipped")
                .into_keys()
                .collect();
            assert_eq!(reached, expected, "{what}: not the clipped slicer's tiles");
            for tile in padded_tiles(std::slice::from_ref(&polygon.exterior)) {
                let one = slicer.tile_polygon(tile, &rings).expect("tile_polygon");
                let one = one.map(|p| p.rings().map(|r| local(tile, r)).collect::<Vec<_>>());
                let expected = reached.binary_search(&tile).is_ok().then(|| to_local(tile, &whole));
                assert_eq!(one, expected, "{what}: tile_polygon differs at {tile:?}");
                let mut within = Vec::new();
                let mut slices = slicer.polygon(&rings).expect("polygon").within(tile, tile);
                while let Some(slice) = slices.next() {
                    let PolygonSlice::Piece(piece) = slice.expect("slice") else {
                        panic!("{what}: an unclipped slicer yielded a fill run");
                    };
                    within.push((piece.tile(), piece.rings().map(|r| local(tile, r)).collect()));
                }
                let expected: Vec<(TileId, Vec<_>)> = expected.into_iter().map(|e| (tile, e)).collect();
                assert_eq!(within, expected, "{what}: within({tile:?}) differs");
            }
            for tile in reached {
                *tiles.entry(tile).or_default() += 1;
            }
        }
        let inputs = polygons.iter().map(|p| support::input_polygon(&p.exterior, &p.holes));
        snapshot("polygons", stem, buffer, inputs.collect(), &tiles);
    }
}

/// Snapshot `inputs` and the buffered box of every reached tile, with how many geometries it holds,
/// into `{kind}/snapshots-unclipped/` (buffer 0 only).
fn snapshot(kind: &str, stem: &str, buffer: u16, mut features: Vec<geojson::Feature>, tiles: &BTreeMap<TileId, u32>) {
    if buffer > 0 {
        return;
    }
    let (e, b) = (EXTENT as i32, i32::from(buffer));
    for (&tile, &count) in tiles {
        let (x0, y0) = (tile.x * e - b, tile.y * e - b);
        let (x1, y1) = (x0 + e + 2 * b, y0 + e + 2 * b);
        let ring = [(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)].map(|(x, y)| Coord { x, y });
        let mut f = support::tile_polygon(&ring, &[], tile);
        f.set_property("geometries", json!(count));
        features.push(f);
    }
    let (dir, name) = (format!("{kind}/snapshots-unclipped"), format!("{stem}.geojson"));
    insta::with_settings!({
        snapshot_path => dir,
        prepend_module_to_snapshot => false,
    }, {
        assert_binary_snapshot!(&name, support::feature_collection_bytes(features));
    });
}

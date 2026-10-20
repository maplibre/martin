//! The baseline clippers (`tests/support/baseline.rs`) do the toolkit's work: per tile, the stripe
//! clipper's output covers the same area (polygons) or length (polylines) as the toolkit's pieces
//! once those are cut to the buffered box (they keep original vertices outside it), and so does
//! `geo`'s overlay. Exact equality is not expected: the baselines compute intersection points, and
//! Sutherland-Hodgman leaves degenerate segments along the box edges.
//!
//! Area is the even-odd area `geo` measures, one polygon at a time, so self-intersecting rings and
//! overlapping holes compare the same way on every side (clipping to a half-plane keeps each inside
//! point's winding number, so the stripe clipper keeps its parity).

#![allow(clippy::pedantic, reason = "test tool")]

use std::collections::BTreeMap;
use std::path::Path;

use geo::{Area as _, BooleanOps as _, Euclidean, Length as _};
use geo_types::{Coord, LineString, MultiLineString, Polygon};
use map_tile_toolkit::{Slicer, TileId};

#[path = "support/baseline.rs"]
mod baseline;
mod support;

use baseline::Pt;
use support::FixturePolygon;

fn pts(ring: &[Coord<i32>], origin: Coord<i32>) -> Vec<Pt> {
    ring.iter()
        .map(|&c| [f64::from(c.x + origin.x), f64::from(c.y + origin.y)])
        .collect()
}

fn line_string(ring: &[Pt]) -> LineString<f64> {
    ring.iter().copied().map(Coord::from).collect()
}

/// The rings as one `geo` polygon: the first as exterior, the rest as interiors (`geo`'s even-odd
/// overlay treats them all alike).
fn polygon(rings: &[Vec<Pt>]) -> Polygon<f64> {
    match rings {
        [] => Polygon::new(LineString(vec![]), vec![]),
        [first, rest @ ..] => Polygon::new(line_string(first), rest.iter().map(|r| line_string(r)).collect()),
    }
}

/// Equal up to `1e-6` of `scale` (the tile box's area or side): `geo`'s overlay snaps to a fixed-point
/// grid sized by its input's extent.
fn assert_close(what: &str, tile: TileId, scale: f64, expected: f64, actual: f64) {
    assert!(
        (expected - actual).abs() <= 1e-6 * scale,
        "{what} in tile {}/{}: expected {expected}, got {actual}",
        tile.x,
        tile.y,
    );
}

/// The tiles to check, column-major: the parts' bounding box grown by the buffer, padded by a tile so
/// empty neighbors are covered too.
fn tiles(parts: &[Vec<Pt>], extent: u32, buffer: u16) -> Vec<TileId> {
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in parts.iter().flatten() {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    let (e, b) = (f64::from(extent), f64::from(buffer));
    let tile = |v: f64| (v / e).floor() as i32;
    let (x0, y0, x1, y1) = (
        tile(lo[0] - b) - 1,
        tile(lo[1] - b) - 1,
        tile(hi[0] + b) + 1,
        tile(hi[1] + b) + 1,
    );
    (x0..=x1)
        .flat_map(|x| (y0..=y1).map(move |y| TileId::new(x, y)))
        .collect()
}

/// Per tile, the area of each of `polygons` (summed) from the toolkit, the stripe clipper (one tile
/// and all tiles), and geo.
fn check_polygons(polygons: &[FixturePolygon], extent: u32, buffer: u16) {
    let zero = Coord { x: 0, y: 0 };
    let rings: Vec<Vec<Vec<Pt>>> = polygons
        .iter()
        .map(|p| {
            std::iter::once(&p.exterior)
                .chain(&p.holes)
                .map(|r| pts(r, zero))
                .collect()
        })
        .collect();
    let tiles = tiles(&rings.concat(), extent, buffer);
    let box_area = (f64::from(extent) + 2.0 * f64::from(buffer)).powi(2);
    let rect = |tile| baseline::tile_rect(tile, extent, buffer).to_polygon();

    // A polygon whose exterior has no area in a tile is dropped there, as the toolkit does, even if
    // one of its holes reaches the tile (a hole outside its shell: invalid, but in the fixtures).
    let area = |tile, parts: &[Vec<Pt>]| rect(tile).intersection(&polygon(parts)).unsigned_area();
    let mut stripe_all = BTreeMap::<TileId, f64>::new();
    for polygon_rings in &rings {
        let mut shell = BTreeMap::new();
        baseline::clip_all(&polygon_rings[..1], &tiles, extent, buffer, true, |t, parts| {
            shell.insert(t, area(t, parts));
        });
        baseline::clip_all(polygon_rings, &tiles, extent, buffer, true, |t, parts| {
            if shell.get(&t).is_some_and(|&a| a > 0.0) {
                *stripe_all.entry(t).or_default() += area(t, parts);
            }
        });
    }

    for &tile in &tiles {
        let origin = tile.origin(extent).expect("origin");
        let (mut toolkit, mut stripe, mut geo) = (0.0, 0.0, 0.0);
        let mut slicer = Slicer::<Coord<i32>>::new(extent, buffer).expect("config");
        for (p, polygon_rings) in polygons.iter().zip(&rings) {
            let one = support::tile_polygons(&mut slicer, std::slice::from_ref(p), tile).expect("clip");
            for piece in one {
                let pieces: Vec<_> = piece.iter().map(|r| pts(r, origin)).collect();
                toolkit += area(tile, &pieces);
            }

            let mut shell = Vec::new();
            baseline::clip_tile(&polygon_rings[..1], tile, extent, buffer, true, &mut shell);
            if area(tile, &shell) > 0.0 {
                let mut out = Vec::new();
                baseline::clip_tile(polygon_rings, tile, extent, buffer, true, &mut out);
                stripe += area(tile, &out);
                geo += area(tile, polygon_rings);
            }
        }
        assert_close("stripe area", tile, box_area, toolkit, stripe);
        let all = stripe_all.get(&tile).copied().unwrap_or(0.0);
        assert_close("stripe-all area", tile, box_area, toolkit, all);
        assert_close("geo area", tile, box_area, toolkit, geo);
    }
}

#[test]
fn polygon_baselines_cover_the_toolkit_area() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/polygons/fixtures");
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .expect("polygon fixtures dir")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "geojson"))
        .collect();
    paths.sort();
    for path in &paths {
        for feature in support::load_polygon_features(path) {
            check_polygons(&feature, support::EXTENT, 0);
            check_polygons(&feature, support::EXTENT, 5);
        }
    }
}

#[test]
fn polygon_baselines_cover_the_toolkit_area_big() {
    for (_, cfg) in support::big_configs() {
        check_polygons(&[support::big_polygon()], cfg.extent, cfg.buffer);
    }
}

fn length(parts: &[Vec<Pt>]) -> f64 {
    parts
        .iter()
        .flat_map(|p| p.windows(2))
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .sum()
}

/// Per tile, the length of `line` from the toolkit, the stripe clipper (one tile and all tiles), and
/// geo.
fn check_line(line: &[Coord<i32>], extent: u32, buffer: u16) {
    let parts = vec![pts(line, Coord { x: 0, y: 0 })];
    let input = MultiLineString(vec![line_string(&parts[0])]);
    let tiles = tiles(&parts, extent, buffer);
    let side = f64::from(extent) + 2.0 * f64::from(buffer);
    let mut stripe_all = BTreeMap::<TileId, f64>::new();
    baseline::clip_all(&parts, &tiles, extent, buffer, false, |t, p| {
        stripe_all.insert(t, length(p));
    });

    for &tile in &tiles {
        let rect = baseline::tile_rect(tile, extent, buffer).to_polygon();
        let origin = tile.origin(extent).expect("origin");
        let slicer = Slicer::<Coord<i32>>::new(extent, buffer).expect("config");
        let runs: Vec<_> = slicer
            .tile_line(tile, line)
            .expect("clip")
            .map(|s| line_string(&pts(&s.run().to_vec(), origin)))
            .collect();
        let toolkit = Euclidean.length(&rect.clip(&MultiLineString(runs), false));

        let mut out = Vec::new();
        baseline::clip_tile(&parts, tile, extent, buffer, false, &mut out);
        assert_close("stripe length", tile, side, toolkit, length(&out));
        let all = stripe_all.get(&tile).copied().unwrap_or(0.0);
        assert_close("stripe-all length", tile, side, toolkit, all);
        let geo = Euclidean.length(&rect.clip(&input, false));
        assert_close("geo length", tile, side, toolkit, geo);
    }
}

#[test]
fn polyline_baselines_cover_the_toolkit_length() {
    for (_, lines) in support::load_all_fixtures() {
        for line in &lines {
            check_line(line, support::EXTENT, 0);
            check_line(line, support::EXTENT, 5);
        }
    }
}

#[test]
fn polyline_baselines_cover_the_toolkit_length_big() {
    for (_, cfg) in support::big_configs() {
        for line in support::lines_of(&support::big_polyline()) {
            check_line(line, cfg.extent, cfg.buffer);
        }
    }
}

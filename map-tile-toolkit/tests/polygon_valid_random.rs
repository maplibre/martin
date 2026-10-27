//! Random **valid** polygons with holes, sliced into every tile they reach: every piece of every one
//! must itself be a valid polygon (OGC, via `geo`), not just look right inside its tile.
//!
//! The polygons are a star-shaped exterior with disjoint star-shaped holes inside it, so they are
//! valid by construction (any that is not, by rounding to the grid, is skipped). The streams are
//! fixed, so a failure reproduces.

#![allow(clippy::pedantic, clippy::print_stdout, reason = "test tool")]

use std::f64::consts::TAU;

use geo::Validation as _;
use geo_types::{Coord, LineString, Polygon};
use map_tile_toolkit::{PolygonSlice, Slicer};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> i64 {
        (self.next() % n) as i64
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64)
    }
}

/// A star-shaped ring around `c` with radii in `r0..=r1`, counter-clockwise (`rev` flips it).
fn star(rng: &mut Rng, c: (i64, i64), r0: i64, r1: i64, n: usize, rev: bool) -> Vec<Coord<i32>> {
    let mut pts: Vec<_> = (0..n)
        .map(|i| {
            let a = (i as f64 + rng.below(100) as f64 / 400.0) / n as f64 * TAU;
            let r = rng.range(r0, r1) as f64;
            Coord {
                x: (c.0 as f64 + r * a.cos()).round() as i32,
                y: (c.1 as f64 + r * a.sin()).round() as i32,
            }
        })
        .collect();
    if rev {
        pts.reverse();
    }
    pts.push(pts[0]);
    pts
}

fn polygon(rings: &[Vec<Coord<i32>>]) -> Polygon<f64> {
    let line = |r: &Vec<Coord<i32>>| {
        LineString::new(
            r.iter()
                .map(|c| Coord {
                    x: f64::from(c.x),
                    y: f64::from(c.y),
                })
                .collect(),
        )
    };
    Polygon::new(line(&rings[0]), rings[1..].iter().map(line).collect())
}

/// A star exterior of radius up to `big` around `(0, 0)` with up to `holes` disjoint star holes.
fn random_polygon(rng: &mut Rng, big: i64, holes: usize) -> Vec<Vec<Coord<i32>>> {
    let n = rng.range(5, 40) as usize;
    let mut rings = vec![star(rng, (0, 0), big * 7 / 10, big, n, false)];
    let mut placed: Vec<(i64, i64, i64)> = Vec::new();
    for _ in 0..holes {
        let r = rng.range(big / 30 + 2, big / 6 + 3);
        let reach = big * 7 / 10 - r - 2;
        if reach < 1 {
            continue;
        }
        let c = (rng.range(-reach, reach), rng.range(-reach, reach));
        let clear = |h: &(i64, i64, i64)| {
            let d = (((c.0 - h.0).pow(2) + (c.1 - h.1).pow(2)) as f64).sqrt();
            d > (r + h.2) as f64 * 1.5 + 4.0
        };
        if (c.0 as f64).hypot(c.1 as f64) + r as f64 * 1.5 < reach as f64 + r as f64 && placed.iter().all(clear) {
            placed.push((c.0, c.1, r));
            let n = rng.range(4, 20) as usize;
            rings.push(star(rng, c, r * 2 / 3, r, n, true));
        }
    }
    rings
}

#[test]
fn pieces_of_random_valid_polygons_are_valid() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let (mut polygons, mut pieces, mut invalid) = (0, 0, Vec::new());
    for (extent, buffer, big) in [(25, 0, 60), (25, 5, 60), (64, 3, 150), (40, 8, 90)] {
        let mut slicer = Slicer::<Coord<i32>>::new(extent, buffer).expect("config");
        for i in 0..250 {
            let rings = random_polygon(&mut rng, big, (i % 6) as usize);
            if polygon(&rings).check_validation().is_err() {
                continue;
            }
            polygons += 1;
            let refs: Vec<&[Coord<i32>]> = rings.iter().map(Vec::as_slice).collect();
            let mut slices = slicer.polygon(&refs).expect("polygon");
            while let Some(slice) = slices.next() {
                let PolygonSlice::Piece(piece) = slice.expect("slice") else {
                    continue;
                };
                pieces += 1;
                let rings: Vec<Vec<Coord<i32>>> = piece.rings().map(|r| r.to_vec()).collect();
                if let Err(e) = polygon(&rings).check_validation() {
                    invalid.push(format!(
                        "extent {extent} buffer {buffer} polygon {i} tile {:?}: {e}",
                        piece.tile()
                    ));
                }
            }
        }
    }
    println!("{polygons} polygons, {pieces} pieces, {} invalid", invalid.len());
    assert!(
        invalid.is_empty(),
        "{} of {pieces} pieces invalid:\n{}",
        invalid.len(),
        invalid.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
}

//! Fuzz `Quantizer::multi_polygon`: a valid float multipolygon must round into valid polygons, even
//! where its parts round onto each other.
//!
//! Each coordinate is an `i16` divided by `1 + scale % 2048`, so vertices land between grid points and
//! parts can be closer together than a unit.
//! Its rounded area must differ from its float area by at most its perimeter plus one.

#![no_main]

use arbitrary::Arbitrary;
use geo::line_measures::LengthMeasurable as _;
use geo::{Area as _, Euclidean, Validation as _};
use geo_types::{Coord, LineString, MultiPolygon, Polygon};
use libfuzzer_sys::fuzz_target;
use map_tile_toolkit::Quantizer;

#[derive(Arbitrary, Debug)]
struct Input {
    /// Coordinates are divided by `1 + scale % 2048`.
    scale: u16,
    /// Each polygon's rings, exterior first.
    polygons: Vec<Vec<Vec<(i16, i16)>>>,
}

fn polygon<R: AsRef<[C]>, C: Copy>(rings: &[R], to_f64: impl Fn(C) -> Coord<f64>) -> Polygon<f64> {
    let line = |ring: &R| LineString::new(ring.as_ref().iter().map(|&c| to_f64(c)).collect());
    Polygon::new(line(&rings[0]), rings[1..].iter().map(line).collect())
}

fn as_f64(c: Coord<i32>) -> Coord<f64> {
    Coord {
        x: f64::from(c.x),
        y: f64::from(c.y),
    }
}

fuzz_target!(|input: Input| {
    let unit = f64::from(1 + input.scale % 2048);
    let polygons: Vec<Vec<Vec<Coord<f64>>>> = input
        .polygons
        .iter()
        .take(4)
        .filter(|rings| !rings.is_empty())
        .map(|rings| {
            rings
                .iter()
                .take(3)
                .map(|ring| {
                    ring.iter()
                        .take(16)
                        .map(|&(x, y)| Coord {
                            x: f64::from(x) / unit,
                            y: f64::from(y) / unit,
                        })
                        .collect()
                })
                .collect()
        })
        .collect();
    if polygons.len() < 2 {
        return;
    }
    let float = MultiPolygon::new(polygons.iter().map(|rings| polygon(rings, |c| c)).collect());
    let ring_area = |r: &LineString<f64>| Polygon::new(r.clone(), vec![]).unsigned_area();
    let rings = || float.iter().flat_map(|p| std::iter::once(p.exterior()).chain(p.interiors()));
    if !rings().all(|r| ring_area(r) > 0.0) || float.check_validation().is_err() {
        return;
    }

    let mut quantizer = Quantizer::new();
    let quantized = quantizer
        .multi_polygon(polygons.iter().map(|rings| rings.iter().map(|r| r.iter().copied())))
        .expect("i16 coordinates fit");
    let rounded: Vec<Vec<Vec<Coord<i32>>>> = quantized.polygons().map(|p| p.map(<[_]>::to_vec).collect()).collect();
    let multi = MultiPolygon::new(rounded.iter().map(|p| polygon(p, as_f64)).collect());
    if let Err(e) = multi.check_validation() {
        panic!("quantizer gave an invalid multipolygon: {e}\nfloat: {float:?}\nquantized: {multi:?}");
    }

    let before = float.unsigned_area();
    let after = multi.unsigned_area();
    let perimeter: f64 = rings().map(|r| r.length(&Euclidean)).sum();
    assert!(
        (before - after).abs() <= perimeter + 1.0,
        "area {before} became {after}, perimeter {perimeter}\nfloat: {float:?}\nquantized: {multi:?}",
    );
});

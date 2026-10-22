//! Fuzz the path a tile generator takes from float geometry to tiles: `Quantizer` rounds a float
//! polygon to the integer grid, and every polygon it gives must be valid, as must every piece the
//! slicer cuts from it.
//!
//! Each coordinate is an `i16` divided by `2^bits`, so vertices land between grid points.
//! The float polygon may be invalid: `make_valid` repairs whatever it gets.
//! A valid float polygon goes through `polygon` or `make_valid`, by the parity of `bits`.
//! Its rounded area must stay within one unit of its perimeter.

#![no_main]

use arbitrary::Arbitrary;
use geo::line_measures::LengthMeasurable as _;
use geo::{Area as _, Euclidean, Validation as _};
use geo_types::{Coord, LineString, MultiPolygon, Polygon};
use libfuzzer_sys::fuzz_target;
use map_tile_toolkit::{PolygonSlice, Quantizer, Slicer};

#[derive(Arbitrary, Debug)]
struct Input {
    extent: u8,
    buffer: u8,
    /// Coordinates are divided by `2^(1 + bits % 6)`.
    bits: u8,
    /// One polygon's rings, exterior first.
    rings: Vec<Vec<(i16, i16)>>,
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
    let unit = f64::from(1_u32 << (1 + input.bits % 6));
    let rings: Vec<Vec<Coord<f64>>> = input
        .rings
        .iter()
        .take(4)
        .map(|ring| {
            ring.iter()
                .take(24)
                .map(|&(x, y)| Coord {
                    x: f64::from(x) / unit,
                    y: f64::from(y) / unit,
                })
                .collect()
        })
        .collect();
    if rings.is_empty() {
        return;
    }
    let float = polygon(&rings, |c| c);
    let ring_area = |r: &LineString<f64>| Polygon::new(r.clone(), vec![]).unsigned_area();
    let before = ring_area(float.exterior()) - float.interiors().iter().map(ring_area).sum::<f64>();
    // `geo` accepts a ring folded flat onto a line, which OGC does not.
    let valid = float.check_validation().is_ok()
        && std::iter::once(float.exterior())
            .chain(float.interiors())
            .all(|r| ring_area(r) > 0.0);

    let mut quantizer = Quantizer::new();
    let float_rings = rings.iter().map(|r| r.iter().copied());
    let quantized = if valid && input.bits % 2 == 0 {
        quantizer.polygon(float_rings)
    } else {
        quantizer.make_valid(float_rings)
    };
    let quantized = quantized.expect("i16 coordinates fit");
    let polygons: Vec<Vec<Vec<Coord<i32>>>> = quantized.polygons().map(|p| p.map(<[_]>::to_vec).collect()).collect();
    let multi = MultiPolygon::new(polygons.iter().map(|p| polygon(p, as_f64)).collect());
    if let Err(e) = multi.check_validation() {
        panic!("quantizer gave an invalid polygon: {e}\nfloat: {float:?}\nquantized: {multi:?}");
    }

    if valid {
        let perimeter: f64 = std::iter::once(float.exterior())
            .chain(float.interiors())
            .map(|r| r.length(&Euclidean))
            .sum();
        let after = multi.unsigned_area();
        assert!(
            (before - after).abs() <= perimeter + 1.0,
            "area {before} became {after}, perimeter {perimeter}\nfloat: {float:?}\nquantized: {multi:?}",
        );
    }

    let Ok(mut slicer) = Slicer::<Coord<i32>>::new(u32::from(input.extent), u16::from(input.buffer)) else {
        return;
    };
    for rings in &polygons {
        let Ok(mut slices) = slicer.polygon(rings) else {
            continue;
        };
        while let Some(slice) = slices.next() {
            let Ok(PolygonSlice::Piece(piece)) = slice else {
                continue;
            };
            let piece_rings: Vec<Vec<Coord<i32>>> = piece.rings().map(|r| r.to_vec()).collect();
            if let Err(e) = polygon(&piece_rings, as_f64).check_validation() {
                panic!(
                    "valid polygon sliced into an invalid piece in {:?}: {e}\nquantized: {rings:?}\npiece: {piece_rings:?}",
                    piece.tile(),
                );
            }
        }
    }
});

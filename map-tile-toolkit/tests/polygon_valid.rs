//! Every piece the clipped polygon slicer gives a tile must itself be a valid polygon whenever the
//! input polygon is - what a consumer that triangulates each piece on its own (MapLibre's earcut
//! over an MVT tile, say) relies on: simple rings, holes inside the exterior and clear of one another.
//!
//! Each fixture in `tests/polygons/fixtures` is checked at buffers 0 and 5, tile by tile across the
//! padded tile span with [`Slicer::tile_polygon`], using `geo`'s OGC validity. Fixtures whose input is
//! not itself valid (deliberately overlapping holes, say) are skipped: nothing is promised for them.

#![allow(clippy::pedantic, reason = "test tool")]

use std::path::Path;

use geo::Validation as _;
use geo_types::{Coord, LineString, Polygon};
use map_tile_toolkit::{Slicer, TileId};

mod support;

use crate::support::{EXTENT, FixturePolygon};

mod files {
    use test_each_file::test_each_path;

    test_each_path! { for ["geojson"] in "./map-tile-toolkit/tests/polygons/fixtures" => super::pieces_are_valid }
}

/// `rings` (exterior first) as an `f64` polygon, which `geo` can validate.
fn as_polygon<R: AsRef<[Coord<i32>]>>(rings: &[R]) -> Polygon<f64> {
    let line = |ring: &R| {
        LineString::new(
            ring.as_ref()
                .iter()
                .map(|c| Coord {
                    x: f64::from(c.x),
                    y: f64::from(c.y),
                })
                .collect(),
        )
    };
    Polygon::new(line(&rings[0]), rings[1..].iter().map(line).collect())
}

fn pieces_are_valid([path]: [&Path; 1]) {
    let polygons: Vec<FixturePolygon> = support::load_polygon_features(path)
        .into_iter()
        .flatten()
        .collect();
    let all_exteriors: Vec<Vec<Coord<i32>>> = polygons.iter().map(|p| p.exterior.clone()).collect();
    let (lo, hi) = support::padded_tile_span(&all_exteriors);

    let mut invalid = Vec::new();
    for buffer in [0, 5] {
        let mut slicer = Slicer::<Coord<i32>>::new(EXTENT, buffer).expect("valid config");
        for (n, polygon) in polygons.iter().enumerate() {
            let input = support::rings(polygon);
            if as_polygon(&input).check_validation().is_err() {
                continue;
            }
            for y in lo.y..=hi.y {
                for x in lo.x..=hi.x {
                    let tile = TileId::new(x, y);
                    let Some(piece) = slicer.tile_polygon(tile, &input).expect("clip") else {
                        continue;
                    };
                    let rings: Vec<Vec<Coord<i32>>> = piece.rings().map(|r| r.to_vec()).collect();
                    if let Err(e) = as_polygon(&rings).check_validation() {
                        invalid.push(format!("buffer {buffer}, polygon {n}, tile {x},{y}: {e}"));
                    }
                }
            }
        }
    }
    assert!(
        invalid.is_empty(),
        "{} invalid piece(s) from a valid polygon:\n{}",
        invalid.len(),
        invalid.join("\n")
    );
}

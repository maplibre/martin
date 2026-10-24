//! `Quantizer` rounds float polygons onto the integer grid and never makes a valid polygon invalid.

use std::fmt::Write as _;

use geo_types::Coord;
use insta::assert_snapshot;
use map_tile_toolkit::{Quantized, Quantizer, TileError};

fn coords<'a>(rings: &'a [&[(f64, f64)]]) -> impl Iterator<Item = impl Iterator<Item = Coord<f64>>> + 'a {
    rings.iter().map(|r| r.iter().map(|&(x, y)| Coord { x, y }))
}

/// One line per polygon, each ring in parentheses.
fn show(quantized: Quantized<'_>) -> String {
    let mut out = String::new();
    for polygon in quantized.polygons() {
        let rings: Vec<String> = polygon
            .map(|r| {
                let coords: Vec<String> = r.iter().map(|c| format!("{} {}", c.x, c.y)).collect();
                format!("({})", coords.join(", "))
            })
            .collect();
        writeln!(out, "{}", rings.join(", ")).expect("writing to a String");
    }
    out
}

fn quantize(rings: &[&[(f64, f64)]]) -> String {
    show(Quantizer::new().polygon(coords(rings)).expect("finite coordinates"))
}

fn make_valid(rings: &[&[(f64, f64)]]) -> String {
    show(Quantizer::new().make_valid(coords(rings)).expect("finite coordinates"))
}

fn multi_polygon(polygons: &[&[&[(f64, f64)]]]) -> String {
    show(
        Quantizer::new()
            .multi_polygon(polygons.iter().map(|rings| coords(rings)))
            .expect("finite coordinates"),
    )
}

#[test]
fn parts_apart_after_rounding_stay_separate() {
    let left = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    let right = [(11.4, 0.0), (20.0, 0.0), (20.0, 10.0), (11.4, 10.0)];
    assert_snapshot!(multi_polygon(&[&[&left], &[&right]]), @"
    (0 0, 10 0, 10 10, 0 10)
    (11 0, 20 0, 20 10, 11 10)
    ");
}

#[test]
fn parts_that_round_onto_one_edge_merge() {
    let left = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    let right = [(10.02, 0.0), (20.0, 0.0), (20.0, 10.0), (10.02, 10.0)];
    assert_snapshot!(multi_polygon(&[&[&left], &[&right]]), @"(0 0, 10 0, 20 0, 20 10, 10 10, 0 10)");
}

#[test]
fn parts_that_cross_far_from_any_vertex_merge_into_their_union() {
    let across = [(0.0, 4.0), (20.0, 4.0), (20.0, 6.0), (0.0, 6.0)];
    let down = [(9.0, -10.0), (11.0, -10.0), (11.0, 20.0), (9.0, 20.0)];
    assert_snapshot!(multi_polygon(&[&[&across], &[&down]]), @"(0 4, 9 4, 9 -10, 11 -10, 11 4, 20 4, 20 6, 11 6, 11 20, 9 20, 9 6, 0 6)");
}

#[test]
fn parts_that_touch_at_two_points_and_overlap_between_them_merge() {
    let wide = [(105.0, 105.0), (105.0, 106.0), (-32.0, 106.0), (0.0, 39.0)];
    let narrow = [(0.0, 0.0), (0.0, 62.0), (-32.0, 106.0)];
    assert_snapshot!(multi_polygon(&[&[&wide], &[&narrow]]), @"(-32 106, 0 0, 0 39, 105 105, 105 106)");
}

#[test]
fn far_part_stays_as_it_is_while_two_near_parts_merge() {
    let left = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    let far = [(40.2, 0.1), (49.7, 0.4), (49.6, 9.5), (40.4, 9.8)];
    let right = [(10.02, 0.0), (20.0, 0.0), (20.0, 10.0), (10.02, 10.0)];
    assert_snapshot!(multi_polygon(&[&[&left], &[&far], &[&right]]), @"
    (0 0, 10 0, 20 0, 20 10, 10 10, 0 10)
    (40 0, 50 0, 50 10, 40 10)
    ");
}

#[test]
fn part_vertex_that_rounds_into_another_part_merges() {
    let upper = [(0.0, 0.0), (10.0, 3.0), (0.0, 3.0)];
    let lower = [(2.0, 0.0), (10.0, 0.0), (5.45, 1.55)];
    assert_snapshot!(multi_polygon(&[&[&upper], &[&lower]]), @"(0 0, 2 0, 10 0, 5 2, 10 3, 0 3)");
}

#[test]
fn island_in_a_hole_stays_an_island_while_other_parts_merge() {
    let lake = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    let shore = [(2.0, 2.0), (8.0, 2.0), (8.0, 8.0), (2.0, 8.0)];
    let island = [(4.0, 4.0), (6.0, 4.0), (6.0, 6.0), (4.0, 6.0)];
    let neighbor = [(10.02, 0.0), (20.0, 0.0), (20.0, 10.0), (10.02, 10.0)];
    assert_snapshot!(multi_polygon(&[&[&lake, &shore], &[&island], &[&neighbor]]), @"
    (0 0, 10 0, 20 0, 20 10, 10 10, 0 10), (2 2, 2 8, 8 8, 8 2)
    (4 4, 6 4, 6 6, 4 6)
    ");
}

#[test]
fn island_that_rounds_onto_its_shore_joins_the_land_around_it() {
    let lake = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    let shore = [(2.0, 2.0), (8.0, 2.0), (8.0, 8.0), (2.0, 8.0)];
    let island = [(2.02, 4.0), (5.0, 4.0), (5.0, 6.0), (2.02, 6.0)];
    assert_snapshot!(multi_polygon(&[&[&lake, &shore], &[&island]]), @"(0 0, 10 0, 10 10, 0 10), (2 2, 2 4, 5 4, 5 6, 2 6, 2 8, 8 8, 8 2)");
}

#[test]
fn part_that_rounds_to_a_line_is_dropped_and_the_others_kept() {
    let line = [(0.0, 0.0), (10.0, 0.3), (20.0, 0.1)];
    let exterior = [(0.2, 2.1), (9.7, 2.4), (9.6, 11.5), (0.4, 11.8)];
    let hole = [(3.2, 5.1), (3.4, 8.7), (6.6, 8.8), (6.9, 5.3)];
    assert_snapshot!(multi_polygon(&[&[&line], &[&exterior, &hole]]), @"(0 2, 10 2, 10 12, 0 12), (3 5, 3 9, 7 9, 7 5)");
}

#[test]
fn polygon_that_rounds_valid_keeps_every_vertex() {
    let exterior = [(0.2, 0.1), (9.7, 0.4), (9.6, 9.5), (0.4, 9.8)];
    let hole = [(3.2, 3.1), (3.4, 6.7), (6.6, 6.8), (6.9, 3.3)];
    assert_snapshot!(quantize(&[&exterior, &hole]), @"(0 0, 10 0, 10 10, 0 10), (3 3, 3 7, 7 7, 7 3)");
}

#[test]
fn clockwise_exterior_and_counter_clockwise_hole_are_reversed() {
    let exterior = [(0.0, 0.0), (0.0, 10.0), (10.0, 10.0), (10.0, 0.0)];
    let hole = [(3.0, 3.0), (7.0, 3.0), (7.0, 7.0), (3.0, 7.0)];
    assert_snapshot!(quantize(&[&exterior, &hole]), @"(10 0, 10 10, 0 10, 0 0), (3 7, 7 7, 7 3, 3 3)");
}

#[test]
fn two_tip_vertices_that_would_cross_merge_into_one() {
    let exterior = [
        (1612.28, 395.41),
        (1617.38, 388.41),
        (1654.28, 308.21),
        (1653.54, 308.55),
        (1618.68, 323.32),
        (1609.48, 368.51),
        (1607.98, 394.02),
    ];
    assert_snapshot!(quantize(&[&exterior]), @"(1608 394, 1609 369, 1619 323, 1654 309, 1617 388, 1612 395)");
}

#[test]
fn sliver_that_rounds_onto_itself_keeps_its_area_off_the_fold() {
    let exterior = [(-32.0, -0.25), (3148.25, 12.25), (-490.25, -0.25), (14.5, 0.0)];
    assert_snapshot!(quantize(&[&exterior]), @"(-490 0, -32 0, 15 0, 3148 12)");
}

#[test]
fn hole_that_rounds_onto_the_exterior_becomes_a_notch() {
    let exterior = [(0.6, 0.0), (10.0, 0.0), (10.0, 10.0), (0.6, 10.0)];
    let hole = [(1.4, 4.0), (1.4, 6.0), (3.0, 5.0)];
    assert_snapshot!(quantize(&[&exterior, &hole]), @"(1 0, 10 0, 10 10, 1 10, 1 6, 3 5, 1 4)");
}

#[test]
fn vertex_closer_to_an_edge_than_fixed_point_resolves_rounds_valid() {
    let exterior = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (5.0, 0.001), (0.0, 10.0)];
    assert_snapshot!(quantize(&[&exterior]), @"
    (0 0, 5 0, 0 10)
    (5 0, 10 0, 10 10)
    ");
}

#[test]
fn holes_closer_than_fixed_point_resolves_take_out_their_union() {
    let exterior = [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)];
    let hole_a = [(2.0, 2.0), (8.0005, 2.0), (8.0005, 12.0), (2.0, 12.0)];
    let hole_b = [(8.0014, 2.0), (14.0, 2.0), (14.0, 12.0), (8.0014, 12.0)];
    assert_snapshot!(quantize(&[&exterior, &hole_a, &hole_b]), @"(0 0, 20 0, 20 20, 0 20), (2 2, 2 12, 8 12, 14 12, 14 2, 8 2)");
}

#[test]
fn self_crossing_exterior_keeps_both_lobes() {
    let exterior = [(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)];
    assert_snapshot!(make_valid(&[&exterior]), @"
    (0 0, 5 5, 0 10)
    (5 5, 10 0, 10 10)
    ");
}

#[test]
fn overlapping_holes_take_out_their_union() {
    let exterior = [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)];
    let hole_a = [(2.0, 2.0), (12.0, 2.0), (12.0, 12.0), (2.0, 12.0)];
    let hole_b = [(8.0, 8.0), (18.0, 8.0), (18.0, 18.0), (8.0, 18.0)];
    assert_snapshot!(make_valid(&[&exterior, &hole_a, &hole_b]), @"(0 0, 20 0, 20 20, 0 20), (2 2, 2 12, 8 12, 8 18, 18 18, 18 8, 12 8, 12 2)");
}

#[test]
fn hole_crossing_the_exterior_takes_out_only_its_inside_part() {
    let exterior = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    let hole = [(8.0, 4.0), (14.0, 4.0), (14.0, 6.0), (8.0, 6.0)];
    assert_snapshot!(make_valid(&[&exterior, &hole]), @"(0 0, 10 0, 10 4, 8 4, 8 6, 10 6, 10 10, 0 10)");
}

#[test]
fn hole_outside_the_exterior_is_ignored() {
    let exterior = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    let hole = [(12.0, 4.0), (14.0, 4.0), (14.0, 6.0), (12.0, 6.0)];
    assert_snapshot!(make_valid(&[&exterior, &hole]), @"(0 0, 10 0, 10 10, 0 10)");
}

#[test]
fn exterior_that_rounds_to_a_line_leaves_nothing() {
    let exterior = [(0.0, 0.0), (10.0, 0.3), (20.0, 0.1)];
    assert_snapshot!(quantize(&[&exterior]), @"");
}

#[test]
fn coordinate_that_is_not_finite_is_an_overflow() {
    let mut quantizer = Quantizer::new();
    let exterior = [
        Coord { x: 0.0, y: 0.0 },
        Coord { x: f64::NAN, y: 1.0 },
        Coord { x: 1.0, y: 0.0 },
    ];
    assert_eq!(quantizer.make_valid([exterior]).err(), Some(TileError::Overflow));
}

#[test]
fn coordinate_that_rounds_past_i32_is_an_overflow() {
    let mut quantizer = Quantizer::new();
    let exterior = [
        Coord { x: 0.0, y: 0.0 },
        Coord {
            x: 2_147_483_647.5,
            y: 1.0,
        },
        Coord { x: 1.0, y: 0.0 },
    ];
    assert_eq!(quantizer.polygon([exterior]).err(), Some(TileError::Overflow));
}

#[test]
fn make_valid_rounds_a_valid_polygon_like_polygon_does() {
    let exterior = [(0.2, 0.1), (9.7, 0.4), (9.6, 9.5), (0.4, 9.8)];
    let hole = [(3.2, 3.1), (3.4, 6.7), (6.6, 6.8), (6.9, 3.3)];
    assert_snapshot!(make_valid(&[&exterior, &hole]), @"(0 0, 10 0, 10 10, 0 10), (3 3, 3 7, 7 7, 7 3)");
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "Quantizer::polygon was given an invalid polygon")]
fn polygon_given_a_self_crossing_exterior_panics_in_debug_builds() {
    let exterior = [(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)];
    quantize(&[&exterior]);
}

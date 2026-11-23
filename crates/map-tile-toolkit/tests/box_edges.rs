//! Every tile's clip box is the closed `[-buffer, extent + buffer]` in its frame, as `ST_AsMVTGeom` clips:
//! geometry on a box edge reaches that tile too.

use geo_types::Coord;
use insta::assert_snapshot;
use map_tile_toolkit::{PolygonSlice, Slicer, TileId};

fn coords(points: &[(i32, i32)]) -> Vec<Coord<i32>> {
    points.iter().map(|&(x, y)| Coord { x, y }).collect()
}

fn show(tile: TileId, vertices: impl Iterator<Item = Coord<i32>>) -> String {
    let vertices: Vec<String> = vertices.map(|c| format!("{} {}", c.x, c.y)).collect();
    format!("{}/{}: {}", tile.x, tile.y, vertices.join(", "))
}

fn points(extent: u32, buffer: u16, points: &[(i32, i32)]) -> String {
    let slicer: Slicer = Slicer::new(extent, buffer).expect("valid config");
    let mut out = Vec::new();
    for point in coords(points) {
        for slice in slicer.point(point).expect("far from the i32 limits") {
            out.push(show(slice.tile(), [slice.point()].into_iter()));
        }
    }
    out.join("\n")
}

fn line(extent: u32, buffer: u16, line: &[(i32, i32)]) -> String {
    let slicer: Slicer = Slicer::new(extent, buffer).expect("valid config");
    let line = coords(line);
    let mut out: Vec<String> = slicer
        .line(&line)
        .expect("far from the i32 limits")
        .map(|slice| show(slice.tile(), slice.run().iter()))
        .collect();
    out.sort();
    out.join("\n")
}

fn polygon(extent: u32, buffer: u16, exterior: &[(i32, i32)]) -> String {
    let mut slicer: Slicer = Slicer::new(extent, buffer).expect("valid config");
    let rings = [coords(exterior)];
    let mut pieces = slicer.polygon(&rings).expect("far from the i32 limits");
    let mut out = Vec::new();
    while let Some(slice) = pieces.next() {
        if let PolygonSlice::Piece(piece) = slice.expect("a small piece") {
            for ring in piece.rings() {
                out.push(show(piece.tile(), ring.iter()));
            }
        }
    }
    out.sort();
    out.join("\n")
}

#[test]
fn point_on_a_shared_tile_edge_reaches_both_tiles() {
    assert_snapshot!(points(10, 0, &[(10, 5)]), @"
    0/0: 10 5
    1/0: 0 5
    ");
}

#[test]
fn points_on_both_buffer_edges_reach_the_neighbor() {
    assert_snapshot!(points(10, 2, &[(8, 5), (12, 5)]), @"
    0/0: 8 5
    1/0: -2 5
    0/0: 12 5
    1/0: 2 5
    ");
}

#[test]
fn point_on_a_shared_tile_corner_reaches_all_four_tiles() {
    assert_snapshot!(points(10, 0, &[(10, 10)]), @"
    0/0: 10 10
    0/1: 10 0
    1/0: 0 10
    1/1: 0 0
    ");
}

#[test]
fn line_along_a_shared_tile_edge_reaches_both_tiles() {
    assert_snapshot!(line(10, 0, &[(10, 2), (10, 8)]), @"
    0/0: 10 2, 10 8
    1/0: 0 2, 0 8
    ");
}

#[test]
fn line_along_the_far_buffer_edge_reaches_the_tile() {
    assert_snapshot!(line(10, 2, &[(12, 3), (12, 7)]), @"
    0/0: 12 3, 12 7
    1/0: 2 3, 2 7
    ");
}

#[test]
fn polygon_touching_a_tile_edge_from_outside_reaches_that_tile() {
    assert_snapshot!(polygon(10, 0, &[(10, 2), (15, 2), (15, 8), (10, 8)]), @"
    0/0: 10 2, 15 2, 15 8, 10 8, 10 2
    1/0: 0 2, 5 2, 5 8, 0 8, 0 2
    ");
}

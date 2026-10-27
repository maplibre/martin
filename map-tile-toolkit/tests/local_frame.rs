//! Every ring a polygon slice lends must describe its own frame truthfully: [`Local::origin`] is the
//! piece's tile origin (`tile · extent`), and [`Local::global`] minus that origin is exactly what
//! iterating the ring yields (less the closing vertex of a ring lent open). Checked for every piece
//! of every `tests/polygons/fixtures/*.geojson` polygon, from both [`Slicer::polygon`] and
//! [`Slicer::tile_polygon`], at buffers 0 and 5.
//!
//! [`Local::origin`]: map_tile_toolkit::Local::origin
//! [`Local::global`]: map_tile_toolkit::Local::global

#![allow(clippy::pedantic, reason = "test tool")]

use std::path::Path;

use geo_types::Coord;
use map_tile_toolkit::{Local, PolygonSlice, Slicer, TileId};

mod support;

use crate::support::EXTENT;

mod files {
    use test_each_file::test_each_path;

    // One test per polygon fixture.
    test_each_path! { for ["geojson"] in "./map-tile-toolkit/tests/polygons/fixtures" => super::local_frame }
}

/// Assert that `ring`, lent for `tile`, reports `tile`'s origin, and that its global vertices are its
/// local ones moved by that origin.
fn check_ring(what: &str, tile: TileId, ring: Local<'_, Coord<i32>>) {
    let origin = tile.origin(EXTENT).expect("tile in range");
    assert_eq!(
        ring.origin(),
        origin,
        "{what}: tile {}/{} ring reports origin {:?}",
        tile.x,
        tile.y,
        ring.origin()
    );
    let local = ring.to_vec();
    let from_global: Vec<_> = ring.global().iter().map(|&g| g - origin).collect();
    assert_eq!(
        &local[..from_global.len()],
        &from_global[..],
        "{what}: tile {}/{} global() {:?} is not local {local:?} moved by origin {origin:?}",
        tile.x,
        tile.y,
        ring.global()
    );
}

fn local_frame([path]: [&Path; 1]) {
    let stem = path.file_stem().and_then(|s| s.to_str()).expect("stem");
    for buffer in [0, 5] {
        let mut slicer = Slicer::<Coord<i32>>::new(EXTENT, buffer).expect("valid config");
        for polygon in support::load_polygon_fixture(path) {
            let rings = support::rings(&polygon);
            let what = format!("{stem} (buffer {buffer}) polygon");
            let mut tiles = Vec::new();
            let mut slices = slicer.polygon(&rings).expect("polygon");
            while let Some(slice) = slices.next() {
                if let PolygonSlice::Piece(piece) = slice.expect("slice") {
                    tiles.push(piece.tile());
                    for ring in piece.rings() {
                        check_ring(&what, piece.tile(), ring);
                    }
                }
            }
            for tile in tiles {
                let piece = slicer.tile_polygon(tile, &rings).expect("tile").expect("piece");
                for ring in piece.rings() {
                    check_ring(&format!("{what} via tile_polygon"), tile, ring);
                }
            }
        }
    }
}

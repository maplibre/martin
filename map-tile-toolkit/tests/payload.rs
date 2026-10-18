//! A custom [`Vertex`] that carries a per-vertex payload (here an M value, which `geo-types` cannot
//! represent) rides through slicing and merging untouched, for lines, points and polygons: positions
//! are localized exactly as for plain `Coord`s, and the payload stays on its vertex. Synthetic polygon
//! corners carry the payload's default.
//!
//! Per-*feature* attributes never reach the slicer: the caller pairs them with each slice (see
//! `examples/mvt_features.rs`).

#![allow(clippy::pedantic, reason = "test tool")]

use std::collections::BTreeMap;

use geo_types::Coord;
use map_tile_toolkit::{Measured, Mosaic, PolygonSlice, Slicer, TileError, TileId, Vertex};

mod support;

use crate::support::EXTENT;

/// Slice one polyline into every tile it touches, each tile's runs flattened. Generic over the vertex
/// type, so the same helper drives both the `Measured` and `Coord` paths.
fn slice_all_runs<V: Vertex>(poly: &[V]) -> BTreeMap<TileId, Vec<Vec<V>>> {
    let slicer = Slicer::<V>::new(10, 0).expect("valid config");
    let mut tiles: BTreeMap<TileId, Vec<Vec<V>>> = BTreeMap::new();
    for s in slicer.line(poly).expect("slice") {
        tiles.entry(s.tile()).or_default().push(s.run().to_vec());
    }
    tiles
}

/// A vertical polyline crossing two tile borders, each vertex tagged with a distinct M value.
fn measured_line() -> Vec<Measured<u32>> {
    vec![
        Measured::new(5, 5, 10),  // tile (0,0)
        Measured::new(5, 15, 20), // tile (0,1)
        Measured::new(5, 25, 30), // tile (0,2)
    ]
}

#[test]
fn line_payload_survives_slicing_and_the_mosaic() {
    let tiles = slice_all_runs(&measured_line());

    // Tile (0,1) sees the whole polyline (both segments touch it). Positions are tile-local (origin
    // (0,10)); the M values are carried through unchanged.
    assert_eq!(
        tiles[&TileId::new(0, 1)],
        vec![vec![
            Measured::new(5, -5, 10),
            Measured::new(5, 5, 20),
            Measured::new(5, 15, 30),
        ]]
    );

    // The positions match the same polyline sliced as plain `Coord`s.
    let plain = vec![
        Coord { x: 5, y: 5 },
        Coord { x: 5, y: 15 },
        Coord { x: 5, y: 25 },
    ];
    let positions: BTreeMap<TileId, Vec<Vec<Coord<i32>>>> = tiles
        .iter()
        .map(|(t, runs)| {
            let runs = runs
                .iter()
                .map(|r| r.iter().map(|v| v.position).collect())
                .collect();
            (*t, runs)
        })
        .collect();
    assert_eq!(positions, slice_all_runs(&plain));

    // Feeding every tile into a mosaic reassembles the original line in the global frame with every M
    // value intact (the duplicated border segments collapse).
    let mut mosaic = Mosaic::new(10).expect("valid config");
    for (tile, runs) in tiles {
        mosaic
            .add(tile, runs.as_slice())
            .expect("self-consistent tiles never conflict");
    }
    let features: Vec<Vec<Measured<u32>>> = mosaic.iter_features().collect();
    assert_eq!(features, vec![measured_line()]);
}

#[test]
fn mosaic_rejects_a_payload_mismatch() {
    // Two tiles share the border segment (5,5)→(5,15), but tile (0,1) carries a different M on the
    // shared vertex — the mosaic rejects it, naming tile (0,0), and stays unchanged.
    let mut mosaic = Mosaic::new(10).expect("valid config");
    mosaic
        .add(
            TileId::new(0, 0),
            &[vec![Measured::new(5, 5, 10), Measured::new(5, 15, 20)]],
        )
        .expect("first tile");
    let bad = mosaic.add(
        TileId::new(0, 1),
        &[vec![Measured::new(5, -5, 10), Measured::new(5, 5, 999)]],
    );
    assert_eq!(bad, Err(TileError::Conflict(vec![TileId::new(0, 0)])));
    assert_eq!(mosaic.len(), 1);
    assert!(mosaic.contains(TileId::new(0, 0)));
}

#[test]
fn point_payload_rides_into_every_tile_of_the_point() {
    let s = Slicer::new(10, 2).expect("config");
    let pts = [Measured::new(9, 5, 'a'), Measured::new(15, 15, 'b')];
    let got: Vec<_> = pts
        .iter()
        .flat_map(|&p| s.point(p).expect("point").map(|p| (p.tile(), p.point())))
        .collect();
    assert_eq!(
        got,
        [
            (TileId::new(0, 0), Measured::new(9, 5, 'a')),
            (TileId::new(1, 0), Measured::new(-1, 5, 'a')),
            (TileId::new(1, 1), Measured::new(5, 5, 'b')),
        ]
    );
}

#[test]
fn polygon_payload_rides_through_pieces_and_fill() {
    // Each vertex carries its index as an M value; the all-tiles pieces equal the single-tile clips.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/polygons/fixtures/big_fill.geojson");
    let polygons = support::load_polygon_fixture(&path);
    let mut m = 0;
    let mut measured = |ring: &[Coord<i32>]| -> Vec<Measured<u32>> {
        ring.iter()
            .map(|c| {
                m += 1;
                Measured::new(c.x, c.y, m)
            })
            .collect()
    };
    let rings: Vec<Vec<Vec<Measured<u32>>>> = polygons
        .iter()
        .map(|p| {
            std::iter::once(measured(&p.exterior))
                .chain(p.holes.iter().map(|h| measured(h)))
                .collect()
        })
        .collect();

    let mut slicer = Slicer::<Measured<u32>>::new(EXTENT, 3).expect("config");
    type Rings = Vec<Vec<Measured<u32>>>;
    let mut pieces: BTreeMap<TileId, Vec<Rings>> = BTreeMap::new();
    let mut runs = 0;
    for polygon in &rings {
        let mut slices = slicer.polygon(polygon).expect("slice");
        while let Some(slice) = slices.next() {
            match slice.expect("slice") {
                PolygonSlice::Piece(piece) => pieces
                    .entry(piece.tile())
                    .or_default()
                    .push(piece.rings().map(|r| r.to_vec()).collect()),
                PolygonSlice::Run(_) => runs += 1,
            }
        }
    }
    assert!(runs > 0, "the fixture has covered tiles");
    for (&tile, got) in &pieces {
        let mut expected: Vec<Rings> = Vec::new();
        for polygon in &rings {
            if let Some(piece) = slicer.tile_polygon(tile, polygon).expect("clip") {
                expected.push(piece.rings().map(|r| r.to_vec()).collect());
            }
        }
        assert_eq!(got, &expected, "{tile:?}");
    }
}

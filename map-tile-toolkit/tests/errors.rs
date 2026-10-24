//! The public API never panics: invalid input yields a typed [`TileError`] instead.

use std::collections::BTreeSet;

use geo_types::Coord;
use map_tile_toolkit::{FillRun, PolygonSlice, Slicer, TileError, TileId};

/// A polyline as a `Vec<Coord<i32>>`.
fn line(coords: Vec<(i32, i32)>) -> Vec<Coord<i32>> {
    coords.into_iter().map(|(x, y)| Coord { x, y }).collect()
}

/// A slicer over `Coord` (the config validation is what these tests probe).
fn all(extent: u32, buffer: u16) -> Result<Slicer<Coord<i32>>, TileError> {
    Slicer::new(extent, buffer)
}

/// Every slice of a line, as `(tile, run)` pairs in yield order.
type Slices = Vec<(TileId, Vec<Coord<i32>>)>;

/// Every slice of a line, in yield order, or the error the call returns.
fn slices(s: &Slicer<Coord<i32>>, l: &[Coord<i32>]) -> Result<Slices, TileError> {
    Ok(s.line(l)?.map(|s| (s.tile(), s.run().to_vec())).collect())
}

/// A polyline that starts cleanly in a tile near `i32::MIN` and then runs to a vertex whose tile's
/// buffered box underflows `i32`. `extent = 4096`, `buffer = 8`: the low vertex is `i32::MIN+8`
/// (itself within buffer of the edge) but its tile's base minus the buffer underflows.
fn overflows_midway() -> Vec<Coord<i32>> {
    line(vec![
        (i32::MIN + 8200, 0), // tile -524286 (base i32::MIN+8192): clean, gets yielded
        (i32::MIN + 8250, 0),
        (i32::MIN + 8, 0), // tile -524288 (base i32::MIN): base - buffer underflows -> Overflow
    ])
}

#[test]
fn invalid_extent() {
    assert_eq!(all(0, 0).err(), Some(TileError::InvalidExtent));
    assert_eq!(all(u32::MAX, 0).err(), Some(TileError::InvalidExtent));
    all(1, 0).unwrap();
    // `extent + buffer` must stay below `i32::MAX`, or no tile's box, grown a unit for polygon corners, could fit.
    all(i32::MAX as u32 - 1, 0).unwrap();
    assert_eq!(all(i32::MAX as u32, 0).err(), Some(TileError::InvalidExtent));
    assert_eq!(
        all(i32::MAX as u32 - u32::from(u16::MAX), u16::MAX).err(),
        Some(TileError::InvalidExtent)
    );
    all(i32::MAX as u32 - u32::from(u16::MAX) - 1, u16::MAX).unwrap();
}

#[test]
fn buffer_too_large() {
    // `buffer` must be strictly less than half the `extent` (i.e. `2*buffer < extent`).
    assert_eq!(all(10, 5).err(), Some(TileError::BufferTooLarge)); // 2*5 == 10, not < 10
    assert_eq!(all(10, 6).err(), Some(TileError::BufferTooLarge));
    all(10, 4).unwrap(); // 2*4 == 8 < 10
    // With extent 1 only a zero buffer is allowed.
    all(1, 0).unwrap();
    assert_eq!(all(2, 1).err(), Some(TileError::BufferTooLarge));
}

#[test]
fn points_at_the_i32_limits_error_instead_of_panicking() {
    // Within the buffer of i32::MAX, the neighbor tile's box leaves i32: the call fails, before any
    // point is yielded.
    let s = all(4096, 8).expect("valid config");
    let at = |x: i32, y: i32| Coord { x, y };
    assert_eq!(s.point(at(i32::MAX, 0)).err(), Some(TileError::Overflow));
    // (0, 0) is within the buffer of three neighbors, so it lands in four tiles.
    assert_eq!(s.point(at(0, 0)).expect("clean point").count(), 4);
    // A tile whose own box overflows is rejected up front.
    assert_eq!(
        s.tile_point(TileId::new(i32::MAX, 0), at(0, 0)).err(),
        Some(TileError::Overflow)
    );
}

#[test]
fn a_far_vertex_overflows_the_local_frame_of_a_tile_it_reaches() {
    // Tile 2^18 of extent 4096 has origin 2^30. The line touches it, but its far end, `i32::MIN`, lies
    // more than an `i32` span before that origin: the call fails rather than yielding a run that
    // cannot be expressed in the tile's local frame.
    let s = all(4096, 0).expect("valid config");
    let l = line(vec![(1 << 30, 0), (i32::MIN, 0)]);
    assert_eq!(
        s.tile_line(TileId::new(1 << 18, 0), &l).err(),
        Some(TileError::Overflow)
    );
    // Seen from the tile at the origin, every vertex fits.
    s.tile_line(TileId::new(0, 0), &l).unwrap();
}

#[test]
fn extreme_tile_errors_instead_of_panicking() {
    let l = line(vec![(0, 0), (10, 10)]);
    let s = all(4096, 0).expect("valid config");
    // A tile whose (buffered) box coordinates overflow i32 -> Overflow, not a panic.
    assert_eq!(
        s.tile_line(TileId::new(i32::MAX, i32::MAX), &l).err(),
        Some(TileError::Overflow)
    );
    assert_eq!(
        s.tile_line(TileId::new(i32::MIN, 0), &l).err(),
        Some(TileError::Overflow)
    );
    // A far-but-representable tile touches nothing -> no slices.
    let far = s
        .tile_line(TileId::new(1000, 1000), &l)
        .expect("no error for a far tile");
    assert_eq!(far.count(), 0);
}

#[test]
fn spanning_many_tiles_is_lazy() {
    let s = all(1, 0).expect("valid config"); // 1 unit per tile
    // A line through 40 000 001 tiles: slices come one at a time, so taking a few costs a few.
    let long = line(vec![(0, 0), (40_000_000, 0)]);
    let first: Vec<_> = s.line(&long).expect("lazy").take(3).collect();
    assert_eq!(first.len(), 3);
    // Only the touched tiles are visited, not how far the line reaches: a diagonal across 100 000
    // tiles (a 7·10⁹-tile bounding box) touches under 250 000.
    let s = all(10, 1).expect("valid config");
    let tiles: BTreeSet<TileId> = slices(&s, &line(vec![(0, 0), (1_000_000, 700_010)]))
        .expect("a long diagonal")
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    assert!(tiles.len() > 100_000 && tiles.len() < 250_000, "{}", tiles.len());
}

#[test]
fn coordinate_overflow_errors() {
    let s = all(4096, 8).expect("valid config");
    // A vertex within `buffer` of i32::MAX: the buffered bound overflows i32 -> Overflow.
    assert_eq!(
        slices(&s, &line(vec![(i32::MAX, 0), (i32::MAX, 10)])).err(),
        Some(TileError::Overflow)
    );
}

#[test]
fn long_polylines_are_accepted() {
    // A polyline has no vertex limit.
    let s = all(1_000_000, 0).expect("valid config");
    let coords: Vec<Coord<i32>> = (0..200_000).map(|i| Coord { x: 1 + i % 8, y: 1 }).collect();
    let runs: Vec<usize> = slices(&s, &coords)
        .expect("200 000 vertices")
        .iter()
        .map(|(_, run)| run.len())
        .collect();
    assert_eq!(runs, [200_000]);
}

#[test]
fn an_overflow_is_reported_before_any_slice() {
    let s = all(4096, 8).expect("valid config");
    // The line's bounds reach a tile whose box leaves `i32`: the call fails, with nothing yielded
    // for the clean part of it.
    let bad = overflows_midway();
    assert_eq!(s.line(&bad).err(), Some(TileError::Overflow));
    // A tile far from it is unaffected: the clean first two vertices on their own are fine.
    let clean = line(bad.iter().take(2).map(|c| (c.x, c.y)).collect());
    s.line(&clean).unwrap();
    // The slicer holds no state, so a later line is unaffected. (`y = 0` is within the buffer of
    // rows -1 and 0, so the clean line lands in two tiles.)
    let good = line(vec![(i32::MIN + 8210, 0), (i32::MIN + 8260, 0)]);
    assert_eq!(slices(&s, &good).expect("clean").len(), 2);
}

#[test]
fn empty_and_degenerate_inputs_are_ok() {
    let s = all(25, 0).expect("valid config");
    // Empty polyline -> no slices, no error.
    assert_eq!(slices(&s, &[]).expect("empty polyline is ok"), []);
    // A single-point polyline touches its own tile but yields no ≥2-vertex run.
    let dot = line(vec![(5, 5)]);
    assert_eq!(slices(&s, &dot).expect("single-point polyline is ok"), []);
    let one = s
        .tile_line(TileId::new(0, 0), &dot)
        .expect("single-point polyline is ok");
    assert_eq!(one.count(), 0);
}

// ---- Polygons ----

/// One polygon (exterior only) from integer corner pairs.
fn poly(coords: Vec<(i32, i32)>) -> Vec<Vec<Coord<i32>>> {
    vec![line(coords)]
}

/// One slice of a polygon, owned: a piece (tile, rings) or a fill run.
#[derive(Debug, PartialEq, Eq)]
enum Item {
    Piece(TileId, Vec<Vec<Coord<i32>>>),
    Run(FillRun),
}

/// Every slice of a polygon, in yield order, until the first error (returned last).
fn poly_items(s: &mut Slicer<Coord<i32>>, rings: &[Vec<Coord<i32>>]) -> (Vec<Item>, Option<TileError>) {
    let mut slices = s.polygon(rings).expect("starts");
    let mut items = Vec::new();
    while let Some(slice) = slices.next() {
        match slice {
            Ok(PolygonSlice::Piece(p)) => items.push(Item::Piece(
                p.tile(),
                p.rings().map(map_tile_toolkit::Local::to_vec).collect(),
            )),
            Ok(PolygonSlice::Run(run)) => items.push(Item::Run(run)),
            Err(e) => return (items, Some(e)),
        }
    }
    (items, None)
}

#[test]
fn polygons_spanning_many_tiles_are_lazy() {
    let mut s = all(1, 0).expect("valid config");
    // A ring whose first edge touches 40 000 001 tiles: slices come one at a time.
    let wide = poly(vec![(0, 0), (40_000_000, 0), (40_000_000, 5)]);
    let mut slices = s.polygon(&wide).expect("lazy");
    for _ in 0..3 {
        assert!(matches!(slices.next(), Some(Ok(_))));
    }

    // Only the tiles an edge touches cost anything, never its bounding box: the large triangle's
    // edges span a 5781² = 33 419 961-tile box but touch only ~40 000 tiles, and covered tiles
    // come as runs: the small triangle alone fills ~180 000 tiles.
    let small = poly(vec![(0, 0), (600, 600), (0, 600)]);
    let large = poly(vec![(0, 0), (5780, 5780), (0, 5780)]);
    let (items, err) = poly_items(&mut s, &small);
    assert_eq!(err, None);
    let filled: usize = items
        .iter()
        .map(|i| match i {
            Item::Run(run) => run.x.len(),
            Item::Piece(..) => 0,
        })
        .sum();
    assert!(filled > 170_000, "{filled}");
    let (items, err) = poly_items(&mut s, &large);
    assert_eq!(err, None);
    let pieces = items.iter().filter(|i| matches!(i, Item::Piece(..))).count();
    assert!(pieces < 50_000, "{pieces}");
}

#[test]
fn a_polygon_overflow_is_reported_by_the_call() {
    // A square whose left edge runs through the column of tiles based at x = i32::MIN (extent 4096,
    // buffer 0): closing its ring there needs a `B⁺` corner one unit left of i32::MIN -> Overflow.
    let (x0, x1) = (i32::MIN + 10, i32::MIN + 6000);
    let bad = poly(vec![(x0, 0), (x1, 0), (x1, 10_000), (x0, 10_000)]);
    let mut s = all(4096, 0).expect("valid config");
    let piece = s.tile_polygon(TileId::new(-524_288, 1), &bad);
    assert_eq!(piece.err(), Some(TileError::Overflow), "the single-tile clip agrees");
    assert_eq!(
        s.polygon(&bad).err(),
        Some(TileError::Overflow),
        "from the call, before any slice"
    );

    // The slicer keeps no state across polygons: a fresh one gives the same for a clean one.
    let clean = poly_items(&mut s, &clean_square());
    assert_eq!(clean.1, None);
    assert_eq!(
        clean,
        poly_items(&mut all(4096, 0).expect("valid config"), &clean_square())
    );
}

fn clean_square() -> Vec<Vec<Coord<i32>>> {
    poly(vec![(5, 5), (9000, 5), (9000, 9000), (5, 9000)])
}

#[test]
fn polygon_degenerate_inputs() {
    let mut s = all(25, 0).expect("valid config");
    // No rings, and a degenerate exterior (whose holes are then ignored): nothing is yielded.
    assert_eq!(poly_items(&mut s, &[]), (vec![], None));
    let no_rings: Vec<Vec<Coord<i32>>> = vec![];
    let degenerate = vec![line(vec![(1, 1), (9, 9), (1, 1)]), line(vec![(2, 2), (8, 2), (8, 8)])];
    assert_eq!(poly_items(&mut s, &degenerate), (vec![], None));
    assert!(s.tile_polygon(TileId::new(0, 0), &degenerate).expect("ok").is_none());
    // Neither an empty polygon nor a ring that is degenerate once its repeated vertices are dropped
    // leaves anything in a tile.
    assert!(s.tile_polygon(TileId::new(0, 0), &no_rings).expect("ok").is_none());
    let repeated = vec![
        line(vec![(2, 2), (60, 2), (60, 60), (2, 60)]),
        line(vec![(30, 30), (30, 30), (31, 31), (31, 31)]),
    ];
    let plain_square = vec![line(vec![(2, 2), (60, 2), (60, 60), (2, 60)])];
    assert_eq!(poly_items(&mut s, &repeated), poly_items(&mut s, &plain_square));
    // A degenerate hole is ignored.
    let square = line(vec![(2, 2), (60, 2), (60, 60), (2, 60)]);
    let with_hole = poly_items(&mut s, &[square.clone(), line(vec![(30, 30), (31, 31)])]);
    let plain = poly_items(&mut s, &[square]);
    assert_eq!(with_hole, plain);
    assert_ne!(plain.0, []);
}

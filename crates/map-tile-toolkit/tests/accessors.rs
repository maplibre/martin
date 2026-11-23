//! The small public surface the fixtures don't reach: config getters, conversions, the slicer's reusable
//! working memory, and the borrowed view a line run lends.

use geo_types::Coord;
use map_tile_toolkit::{Mosaic, PolygonMosaic, PolygonSlice, Slicer, TileId};

#[test]
fn getters_and_conversions() {
    let s = Slicer::<Coord<i32>>::new(25, 4).expect("valid config");
    assert_eq!((s.extent(), s.buffer()), (25, 4));
    let m = Mosaic::<Coord<i32>>::new(4096).expect("valid config");
    assert_eq!(m.extent(), 4096);
    let m = PolygonMosaic::<Coord<i32>>::new(4096, 64).expect("valid config");
    assert_eq!((m.extent(), m.buffer()), (4096, 64));
    assert_eq!(TileId::from((3, -7)), TileId::new(3, -7));
}

#[test]
fn slicer_scratch_is_not_part_of_its_value() {
    let mut s = Slicer::<Coord<i32>>::new(25, 4).expect("valid config");
    let fresh = s.clone();
    // A square crossing tile edges, so the polygon slicing uses (and keeps) its working memory.
    let square = [[(40, 80), (65, 80), (65, 90), (40, 90)].map(|(x, y)| Coord { x, y })];
    let count = |s: &mut Slicer<Coord<i32>>| {
        let mut slices = s.polygon(&square).expect("polygon");
        let mut pieces = 0;
        while let Some(slice) = slices.next() {
            if let PolygonSlice::Piece(piece) = slice.expect("polygon") {
                // Views are `Copy`, and cloning one explicitly gives the same view.
                assert_eq!(Clone::clone(&piece).rings().count(), piece.rings().count());
                pieces += 1;
            }
        }
        pieces
    };
    let pieces = count(&mut s);
    assert!(pieces > 1);
    // That working memory is not part of the slicer's value: it compares equal to a fresh one, a
    // clone starts without it, and it does not show in the debug output's fields.
    assert_eq!(s, fresh);
    assert_eq!(s.clone(), fresh);
    assert!(format!("{s:?}").contains("PolyScratch"));
    // Releasing it leaves a slicer that works the same.
    s.shrink_to_fit();
    assert_eq!(count(&mut s), pieces);
}

#[test]
fn a_run_lends_its_input_and_origin() {
    let s = Slicer::<Coord<i32>>::new(25, 0).expect("valid config");
    let line = [Coord { x: 30, y: 5 }, Coord { x: 40, y: 5 }];
    let slice = s.line(&line).expect("line").next().expect("one run");
    let run = slice.run();
    // The borrowed input in the global frame, the origin of the tile, and the local vertices (the
    // `IntoIterator` impls for both the value and a reference agree).
    assert_eq!(run.global(), &line[..]);
    assert_eq!(run.origin(), Coord { x: 25, y: 0 });
    let local = vec![Coord { x: 5, y: 5 }, Coord { x: 15, y: 5 }];
    assert_eq!(run.to_vec(), local);
    assert_eq!((&run).into_iter().collect::<Vec<_>>(), local);
    assert_eq!(run.into_iter().collect::<Vec<_>>(), local);
}

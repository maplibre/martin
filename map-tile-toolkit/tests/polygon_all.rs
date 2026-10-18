//! [`Slicer::polygon`] against its oracle, [`Slicer::tile_polygon`]: for every tile of the
//! reachable span, the pieces and fill rings it yields there (in polygon order) must equal what the
//! single-tile slicer produces, and every other tile must be one it leaves empty. (The fixtures'
//! snapshots, shared by both, are in `clip_polygon.rs`.)
//!
//! Also here: fill coverage against an independent `geo` point-in-polygon of each tile center;
//! reassembly of the edge tiles with [`PolygonMosaic`]; all of it again under every rotation and
//! mirror image of the fixtures (planetiler's tricky hole cases among them); and the large-input bounds.

#![allow(clippy::pedantic, reason = "test tool")]

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use geo::Contains as _;
use geo_types::{Coord, LineString, Point, Polygon};
use map_tile_toolkit::{FillRun, PolygonMosaic, PolygonSlice, Slicer, TileId};

mod support;

use crate::support::{EXTENT, FixturePolygon};

/// One polygon's rings (exterior first), tile-local.
type Rings = Vec<Vec<Coord<i32>>>;

/// A tile's pieces: per polygon, its rings.
type Pieces = Vec<Rings>;

/// What [`Slicer::tile_polygon`] yields for `tile`.
fn one_tile(polygons: &[FixturePolygon], extent: u32, buffer: u16, tile: TileId) -> Pieces {
    let mut slicer = Slicer::<Coord<i32>>::new(extent, buffer).expect("config");
    support::tile_polygons(&mut slicer, polygons, tile).expect("clip")
}

/// One feature's output per tile, each with the polygon it comes from: edge-tile pieces, and the fill
/// rings of the runs covering the tile. Polygons are sliced independently, so where polygons overlap
/// one tile can hold both.
#[derive(Debug, PartialEq, Eq)]
struct Sliced {
    tiles: BTreeMap<TileId, Vec<(u32, Rings)>>,
    fills: BTreeMap<TileId, Vec<(u32, Rings)>>,
}

impl Sliced {
    /// Everything the feature puts in `tile`, in polygon order: what the single-tile slicer yields.
    fn content(&self, tile: TileId) -> Pieces {
        let mut all: Vec<&(u32, Rings)> = self
            .tiles
            .get(&tile)
            .into_iter()
            .chain(self.fills.get(&tile))
            .flatten()
            .collect();
        all.sort_by_key(|(polygon, _)| *polygon);
        all.into_iter().map(|(_, rings)| rings.clone()).collect()
    }
}

/// A multipolygon feature sliced polygon by polygon with `polygon`, limited to `window` with
/// [`PolygonSlices::within`](map_tile_toolkit::PolygonSlices::within) when given, checking the
/// structural promises along the way: for each polygon, edge tiles in row-major order and each once,
/// runs non-empty, row-major, disjoint and maximal, and no tile both an edge tile and filled.
fn all_tiles(
    polygons: &[FixturePolygon],
    extent: u32,
    buffer: u16,
    window: Option<(TileId, TileId)>,
) -> Sliced {
    let mut slicer = Slicer::<Coord<i32>>::new(extent, buffer).expect("config");
    let mut sliced = Sliced {
        tiles: BTreeMap::new(),
        fills: BTreeMap::new(),
    };
    for (polygon, fixture) in (0_u32..).zip(polygons) {
        let input = support::rings(fixture);
        let mut slices = slicer.polygon(&input).expect("slice");
        if let Some((min, max)) = window {
            slices = slices.within(min, max);
        }
        // This polygon's edge tiles and runs, in yield order.
        let mut edges: Vec<TileId> = Vec::new();
        let mut runs: Vec<FillRun> = Vec::new();
        while let Some(slice) = slices.next() {
            match slice.expect("slice") {
                PolygonSlice::Piece(piece) => {
                    let rings: Rings = piece.rings().map(|r| r.to_vec()).collect();
                    assert!(!rings.is_empty(), "a piece always carries geometry");
                    edges.push(piece.tile());
                    sliced
                        .tiles
                        .entry(piece.tile())
                        .or_default()
                        .push((polygon, rings));
                }
                PolygonSlice::Run(run) => runs.push(run),
            }
        }
        assert!(
            edges.is_sorted_by_key(|t| (t.y, t.x)),
            "polygon {polygon}: edge tiles in row-major order"
        );
        assert!(
            edges.windows(2).all(|w| w[0] != w[1]),
            "polygon {polygon}: an edge tile listed twice"
        );
        for (i, run) in runs.iter().enumerate() {
            assert!(!run.x.is_empty(), "empty run {run:?}");
            assert!(
                runs[i + 1..]
                    .iter()
                    .all(|r| (r.y, r.x.start) > (run.y, run.x.end)),
                "a polygon's runs are row-major, disjoint and maximal: {run:?}"
            );
            let ring = slices.fill_ring(run).to_vec();
            for x in run.x.clone() {
                let tile = TileId::new(x, run.y);
                assert!(
                    !edges.contains(&tile),
                    "{tile:?} is both edge and fill of polygon {polygon}"
                );
                sliced
                    .fills
                    .entry(tile)
                    .or_default()
                    .push((polygon, vec![ring.clone()]));
            }
        }
    }
    sliced
}

/// Inclusive tile span every piece can reach (one tile of padding covers any buffer < extent / 2).
fn span(polygons: &[FixturePolygon], extent: u32) -> (TileId, TileId) {
    let e = extent as i32;
    let (mut lo, mut hi) = ((i32::MAX, i32::MAX), (i32::MIN, i32::MIN));
    for c in polygons
        .iter()
        .flat_map(|p| p.exterior.iter().chain(p.holes.iter().flatten()))
    {
        let (x, y) = (c.x.div_euclid(e), c.y.div_euclid(e));
        lo = (lo.0.min(x), lo.1.min(y));
        hi = (hi.0.max(x), hi.1.max(y));
    }
    (
        TileId::new(lo.0 - 1, lo.1 - 1),
        TileId::new(hi.0 + 1, hi.1 + 1),
    )
}

/// Assert the all-tiles slicer agrees with the single-tile oracle on every tile of the span.
fn check(polygons: &[FixturePolygon], extent: u32, buffer: u16, label: &str) {
    let all = all_tiles(polygons, extent, buffer, None);
    // Consecutive duplicate vertices are dropped, so repeating every vertex changes nothing.
    let dup = |ring: &[Coord<i32>]| ring.iter().flat_map(|&c| [c, c]).collect::<Vec<_>>();
    let duped: Vec<FixturePolygon> = polygons
        .iter()
        .map(|p| FixturePolygon {
            exterior: dup(&p.exterior),
            holes: p.holes.iter().map(|h| dup(h)).collect(),
        })
        .collect();
    assert_eq!(
        all,
        all_tiles(&duped, extent, buffer, None),
        "{label}: duplicated vertices"
    );
    let (lo, hi) = span(polygons, extent);
    let in_span = |t: &TileId| (lo.x..=hi.x).contains(&t.x) && (lo.y..=hi.y).contains(&t.y);
    assert!(
        all.fills.keys().all(in_span),
        "{label}: a fill run escapes the span"
    );
    check_windows(&all, polygons, extent, buffer, label);
    for y in lo.y..=hi.y {
        for x in lo.x..=hi.x {
            let tile = TileId::new(x, y);
            let one = one_tile(polygons, extent, buffer, tile);
            let at = format!("{label}: extent {extent} buffer {buffer} tile {tile:?}");
            assert_eq!(all.content(tile), one, "{at}");
        }
    }
}

/// Sliced within a window, the feature yields exactly its unwindowed slices in the window's tiles:
/// for a single tile at each corner and the center of the span, a row and a column through it, each
/// quadrant, and a window reaching past the span.
fn check_windows(all: &Sliced, polygons: &[FixturePolygon], extent: u32, buffer: u16, label: &str) {
    let (lo, hi) = span(polygons, extent);
    let mid = TileId::new(lo.x + (hi.x - lo.x) / 2, lo.y + (hi.y - lo.y) / 2);
    let windows = [
        (lo, lo),
        (hi, hi),
        (mid, mid),
        (TileId::new(lo.x, mid.y), TileId::new(hi.x, mid.y)),
        (TileId::new(mid.x, lo.y), TileId::new(mid.x, hi.y)),
        (lo, mid),
        (mid, hi),
        (TileId::new(lo.x, mid.y), TileId::new(mid.x, hi.y)),
        (TileId::new(mid.x, lo.y), TileId::new(hi.x, mid.y)),
        (
            TileId::new(lo.x - 3, lo.y - 3),
            TileId::new(hi.x + 3, hi.y + 3),
        ),
    ];
    for (min, max) in windows {
        let inside = |t: &TileId| (min.x..=max.x).contains(&t.x) && (min.y..=max.y).contains(&t.y);
        let keep = |tiles: &BTreeMap<TileId, Vec<(u32, Rings)>>| {
            tiles
                .iter()
                .filter(|(t, _)| inside(t))
                .map(|(t, v)| (*t, v.clone()))
                .collect()
        };
        let expected = Sliced {
            tiles: keep(&all.tiles),
            fills: keep(&all.fills),
        };
        assert_eq!(
            all_tiles(polygons, extent, buffer, Some((min, max))),
            expected,
            "{label}: extent {extent} buffer {buffer} window {min:?}..={max:?}"
        );
    }
}

mod files {
    use test_each_file::test_each_path;

    test_each_path! { for ["geojson"] in "./map-tile-toolkit/tests/polygons/fixtures" => super::fixture }
}

fn fixture([path]: [&Path; 1]) {
    fixture_matches_one(path);
    fixture_fill_matches_geo(path);
    fixture_reassembles(path);
    fixture_symmetries(path);
}

fn fixture_matches_one(path: &Path) {
    let polygons = support::load_polygon_fixture(path);
    let stem = path.file_stem().and_then(|s| s.to_str()).expect("stem");
    for buffer in [0, 5] {
        check(&polygons, EXTENT, buffer, stem);
    }
    // Finer grids make the same geometry wrap many more tiles.
    for (extent, buffer) in [(7, 0), (7, 3), (4, 1), (1, 0)] {
        check(&polygons, extent, buffer, stem);
    }
}

/// The tile center (a point of its core cell) in the global frame.
fn center(tile: TileId, extent: u32) -> Point<f64> {
    let e = extent as i32;
    Point::new(f64::from(tile.x * e + e / 2), f64::from(tile.y * e + e / 2))
}

fn geo_polygon(p: &FixturePolygon) -> Polygon<f64> {
    let ring = |r: &[Coord<i32>]| {
        LineString::from(
            r.iter()
                .map(|c| (f64::from(c.x), f64::from(c.y)))
                .collect::<Vec<_>>(),
        )
    };
    Polygon::new(ring(&p.exterior), p.holes.iter().map(|h| ring(h)).collect())
}

/// Every tile no edge touches is filled iff its center lies in some polygon, by `geo`'s independent
/// point-in-polygon.
fn fixture_fill_matches_geo(path: &Path) {
    let polygons = support::load_polygon_fixture(path);
    let geo: Vec<Polygon<f64>> = polygons.iter().map(geo_polygon).collect();
    for (extent, buffer) in [(EXTENT, 0), (EXTENT, 5), (7, 1), (3, 0)] {
        let all = all_tiles(&polygons, extent, buffer, None);
        let (lo, hi) = span(&polygons, extent);
        for y in lo.y..=hi.y {
            for x in lo.x..=hi.x {
                let tile = TileId::new(x, y);
                if all.tiles.contains_key(&tile) {
                    continue;
                }
                let inside = geo.iter().any(|p| p.contains(&center(tile, extent)));
                assert_eq!(
                    all.fills.contains_key(&tile),
                    inside,
                    "{}: extent {extent} buffer {buffer} tile {tile:?}",
                    path.display()
                );
            }
        }
    }
}

/// Directed-edge set of closed rings, skipping zero-length edges.
fn edge_set<'a>(
    rings: impl IntoIterator<Item = &'a [Coord<i32>]>,
) -> HashSet<(Coord<i32>, Coord<i32>)> {
    rings
        .into_iter()
        .flat_map(|r| r.windows(2))
        .filter(|w| w[0] != w[1])
        .map(|w| (w[0], w[1]))
        .collect()
}

/// The edge tiles of every feature reassemble with [`PolygonMosaic`] into the input rings' edges
/// (fill runs carry no original edge, so they are not needed).
fn fixture_reassembles(path: &Path) {
    let features = support::load_polygon_features(path);
    let stem = path.file_stem().and_then(|s| s.to_str()).expect("stem");
    for buffer in [0, 5] {
        reassembles(&features, EXTENT, buffer, stem);
    }
}

/// Fixtures with a hole outside its shell (an invalid polygon): a tile the shell does not reach
/// drops that hole, so only the shell's edges are sure to come back.
const HOLE_OUTSIDE_SHELL: &[&str] = &["only_hole_touches_other_tile"];

/// Slice `features` polygon by polygon with [`Slicer::polygon`], feed every edge tile to a [`PolygonMosaic`], and
/// assert the result has exactly the input rings' edges (for a fixture in [`HOLE_OUTSIDE_SHELL`]:
/// every shell edge, and nothing that is not an input edge).
fn reassembles(features: &[Vec<FixturePolygon>], extent: u32, buffer: u16, label: &str) {
    let input: Vec<&[Coord<i32>]> = features.iter().flatten().flat_map(support::rings).collect();
    let mut slicer = Slicer::<Coord<i32>>::new(extent, buffer).expect("config");
    let mut per_tile: BTreeMap<TileId, Vec<Vec<Coord<i32>>>> = BTreeMap::new();
    for polygon in features.iter().flatten() {
        let input = support::rings(polygon);
        let mut slices = slicer.polygon(&input).expect("slice");
        // Edge-tile pieces only: fill runs carry no original edge, so the mosaic needs none of them.
        while let Some(slice) = slices.next() {
            if let PolygonSlice::Piece(piece) = slice.expect("slice") {
                per_tile
                    .entry(piece.tile())
                    .or_default()
                    .extend(piece.rings().map(|r| r.to_vec()));
            }
        }
    }
    let mut mosaic = PolygonMosaic::<Coord<i32>>::new(extent, buffer).expect("config");
    for (tile, rings) in &per_tile {
        mosaic
            .add(*tile, rings)
            .expect("the slicer's own tiles never conflict");
    }
    let rebuilt: Vec<Vec<Coord<i32>>> = mosaic.iter_features().collect();
    let rebuilt = edge_set(rebuilt.iter().map(Vec::as_slice));
    let input = edge_set(input.iter().copied());
    let at = format!("{label}: reassembly at extent {extent} buffer {buffer}");
    if HOLE_OUTSIDE_SHELL
        .iter()
        .any(|stem| label.starts_with(stem))
    {
        let shells = edge_set(features.iter().flatten().map(|p| p.exterior.as_slice()));
        assert!(rebuilt.is_subset(&input), "{at}: invented edges");
        assert!(shells.is_subset(&rebuilt), "{at}: lost shell edges");
    } else {
        assert_eq!(rebuilt, input, "{at}");
    }
}

/// The eight symmetries of the tile grid: `k % 4` quarter turns about the origin, after a mirror
/// across the y axis when `k >= 4`. All are exact in integers and map every tile's buffered box onto
/// another's, so slicing a transformed polygon must give the transformed tiles. A single mirror flips
/// the winding, so its rings are reversed to keep exteriors and holes wound as they were.
fn dihedral(polygons: &[FixturePolygon], k: u8) -> Vec<FixturePolygon> {
    let mirror = k >= 4;
    let map = |c: &Coord<i32>| {
        let mut c = if mirror {
            Coord { x: -c.x, y: c.y }
        } else {
            *c
        };
        for _ in 0..k % 4 {
            c = Coord { x: -c.y, y: c.x };
        }
        c
    };
    let ring = |r: &[Coord<i32>]| {
        let mut r: Vec<Coord<i32>> = r.iter().map(map).collect();
        if mirror {
            r.reverse();
        }
        r
    };
    let out: Vec<FixturePolygon> = polygons
        .iter()
        .map(|p| FixturePolygon {
            exterior: ring(&p.exterior),
            holes: p.holes.iter().map(|h| ring(h)).collect(),
        })
        .collect();
    let windings = |ps: &[FixturePolygon]| -> Vec<bool> {
        ps.iter()
            .flat_map(support::rings)
            .map(|r| twice_area(r) > 0)
            .collect()
    };
    assert_eq!(
        windings(&out),
        windings(polygons),
        "symmetry {k} keeps winding"
    );
    out
}

/// Twice the signed (shoelace) area of a closed or open ring.
fn twice_area(ring: &[Coord<i32>]) -> i64 {
    let n = ring.len();
    (0..n)
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            i64::from(a.x) * i64::from(b.y) - i64::from(b.x) * i64::from(a.y)
        })
        .sum()
}

/// Every rotation and mirror image of a fixture still matches the single-tile oracle (fill rings
/// included) and still reassembles, at both snapshot buffers and on two finer grids - tiles of 10
/// put boundaries through many fixture vertices, tiles of 7 through few. Each symmetry walks rows
/// and crossings in a different order, so this catches tie-breaks that hold only one way round.
fn fixture_symmetries(path: &Path) {
    let features = support::load_polygon_features(path);
    let stem = path.file_stem().and_then(|s| s.to_str()).expect("stem");
    for k in 0..8 {
        let features: Vec<Vec<FixturePolygon>> = features.iter().map(|f| dihedral(f, k)).collect();
        let polygons: Vec<FixturePolygon> = features.iter().flatten().cloned().collect();
        let label = format!("{stem} (symmetry {k})");
        for (extent, buffer) in [(EXTENT, 0), (EXTENT, 5), (10, 2), (7, 3)] {
            check(&polygons, extent, buffer, &label);
            reassembles(&features, extent, buffer, &label);
        }
    }
}

/// One polygon sliced into every tile: its edge tiles' pieces, its fill runs, and its fill ring.
fn slice_one_polygon(
    polygon: &FixturePolygon,
    extent: u32,
    buffer: u16,
) -> (BTreeMap<TileId, Pieces>, Vec<FillRun>, Vec<Coord<i32>>) {
    let mut slicer = Slicer::<Coord<i32>>::new(extent, buffer).expect("config");
    let input = support::rings(polygon);
    let mut slices = slicer.polygon(&input).expect("slice");
    let (mut tiles, mut runs) = (BTreeMap::new(), Vec::new());
    while let Some(slice) = slices.next() {
        match slice.expect("slice") {
            PolygonSlice::Piece(piece) => {
                let rings: Rings = piece.rings().map(|r| r.to_vec()).collect();
                assert!(tiles.insert(piece.tile(), vec![rings]).is_none());
            }
            PolygonSlice::Run(run) => runs.push(run),
        }
    }
    let ring = runs
        .first()
        .map(|run| slices.fill_ring(run).to_vec())
        .unwrap_or_default();
    (tiles, runs, ring)
}

/// A z14-like ocean: a square spanning 16384 × 16384 tiles of extent 4096 (2^28 tiles) with a
/// 21 × 21-tile hole. Interior tiles must come back as one or two runs per row - bounded memory and
/// time proportional to the perimeter and the rows, none per covered tile.
#[test]
fn huge_polygon_fills_by_runs() {
    const N: i32 = 1 << 14;
    const E: i32 = 4096;
    // The exterior sits 100 units inside the outer tiles; the hole's edges sit mid-tile in
    // columns/rows N/2 ± 10, so it has 19 × 19 tiles strictly inside.
    let polygon = support::huge_square();
    let (tiles, runs, fill_ring) = slice_one_polygon(&polygon, E as u32, 64);
    assert_eq!(tiles.len(), 4 * N as usize - 4 + (21 * 21 - 19 * 19));
    assert_eq!(
        runs.len(),
        (N - 2) as usize + 21,
        "one run per row, two beside the hole"
    );
    let filled: u64 = runs.iter().map(|r| r.x.len() as u64).sum();
    assert_eq!(filled, u64::from((N - 2) as u32).pow(2) - 21 * 21);

    // Spot-check against the single-tile oracle: an edge tile of each ring, a filled tile, and a
    // tile inside the hole.
    let one = |tile| one_tile(std::slice::from_ref(&polygon), E as u32, 64, tile);
    for tile in [
        TileId::new(0, 0),
        TileId::new(N - 1, 77),
        TileId::new(N / 2 - 10, N / 2),
    ] {
        assert_eq!(tiles[&tile], one(tile), "{tile:?}");
    }
    let fill = TileId::new(5, 9);
    assert!(
        runs.iter().any(|r| r.y == fill.y && r.x.contains(&fill.x)),
        "a run covers the tile"
    );
    assert_eq!(one(fill), vec![vec![fill_ring.clone()]]);
    assert!(one(TileId::new(N / 2, N / 2)).is_empty(), "inside the hole");
}

/// A z14-scale polygon with a long diagonal edge - the [`support::diagonal_triangle`] over 16384 ×
/// 16384 tiles of extent 4096 - slices, its edge tiles proportional to the perimeter, and around a
/// sample of edge tiles every tile agrees with [`Slicer::tile_polygon`]: an edge tile's pieces
/// match, a filled tile is one it fills with the fill ring, and any other tile is one it leaves empty.
#[test]
fn long_diagonal_edge_slices_at_z14() {
    const N: i32 = 1 << 14;
    let extent = support::DIAGONAL_EXTENT as u32;
    let polygon = support::diagonal_triangle(N);
    let (tiles, runs, fill_ring) = slice_one_polygon(&polygon, extent, 64);
    assert!(tiles.len() < 8 * N as usize, "{}", tiles.len());
    let mut rows: BTreeMap<i32, Vec<FillRun>> = BTreeMap::new();
    for r in runs {
        rows.entry(r.y).or_default().push(r);
    }
    let filled_tiles: u64 = rows.values().flatten().map(|r| r.x.len() as u64).sum();
    let half = u64::from((N - 2) as u32).pow(2) / 2;
    assert!(filled_tiles.abs_diff(half) < 2 * N as u64, "{filled_tiles}");
    let fill_of = |t: TileId| {
        let runs = rows.get(&t.y)?;
        runs.iter().find(|r| r.x.contains(&t.x))
    };

    let one = |tile| one_tile(std::slice::from_ref(&polygon), extent, 64, tile);
    for t in tiles.keys().step_by(101) {
        for (dx, dy) in (-1..=1).flat_map(|dx| (-1..=1).map(move |dy| (dx, dy))) {
            let tile = TileId::new(t.x + dx, t.y + dy);
            let expected = one(tile);
            match (tiles.get(&tile), fill_of(tile)) {
                (Some(pieces), _) => assert_eq!(pieces, &expected, "{tile:?}"),
                (None, Some(_)) => assert_eq!(expected, vec![vec![fill_ring.clone()]], "{tile:?}"),
                (None, None) => assert!(expected.is_empty(), "{tile:?}"),
            }
        }
    }
}

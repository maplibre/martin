//! Shared helpers for the snapshot tests and the profiling example: GeoJSON fixture loading/parsing
//! and feature building. Included by `tests/clip_polyline.rs` (`mod support;`) and by
//! `examples/profile.rs` (via `#[path = "../tests/support/mod.rs"]`).

#![allow(
    dead_code,
    clippy::panic,
    clippy::wildcard_enum_match_arm,
    reason = "shared across the test and example crates; not every helper is used in each"
)]

use std::collections::BTreeMap;
use std::f64::consts::TAU;
use std::fs;
use std::path::Path;

use geo_types::{Coord, Geometry, LineString, MultiLineString, MultiPoint, Point, Polygon};
use geojson::{Feature, FeatureCollection, GeoJson, GeometryValue, JsonObject, JsonValue};
use map_tile_toolkit::{PolygonSlice, Slicer, TileError, TileId};
use serde_json::json;

pub const EXTENT: u32 = 25;

/// A slicer config (extent + buffer) shared by the tests and examples, from which each caller
/// builds a [`Slicer`] (or a polygon slicer).
#[derive(Clone, Copy)]
pub struct Cfg {
    pub extent: u32,
    pub buffer: u16,
}

impl Cfg {
    /// A slicer for this config (panics on a bad literal config).
    #[must_use]
    pub fn slicer(self) -> Slicer<Coord<i32>> {
        Slicer::new(self.extent, self.buffer).expect("invalid slicer config in test support")
    }
}

/// One polygon's rings in one tile (exterior first), tile-local.
pub type Rings = Vec<Vec<Coord<i32>>>;

/// A multipolygon feature's slices in every tile, the way a consumer that wants them per tile renders
/// them: each polygon is sliced on its own, and each tile's pieces, and in each tile of a fill run its
/// polygon's fill ring, are listed with their polygon's index, in polygon order (each polygon has at
/// most one item per tile).
///
/// # Errors
///
/// The first error the slicer yields.
pub fn slice_polygons(
    slicer: &mut Slicer<Coord<i32>>,
    polygons: &[FixturePolygon],
) -> Result<BTreeMap<TileId, Vec<(u32, Rings)>>, TileError> {
    let mut tiles: BTreeMap<TileId, Vec<(u32, Rings)>> = BTreeMap::new();
    for (index, polygon) in (0_u32..).zip(polygons) {
        let input = rings(polygon);
        let mut slices = slicer.polygon(&input)?;
        let mut runs = Vec::new();
        while let Some(slice) = slices.next() {
            match slice? {
                PolygonSlice::Piece(piece) => tiles
                    .entry(piece.tile())
                    .or_default()
                    .push((index, piece.rings().map(|r| r.to_vec()).collect())),
                PolygonSlice::Run(run) => runs.push(run),
            }
        }
        for run in runs {
            let ring = slices.fill_ring(&run).to_vec();
            for x in run.x.clone() {
                tiles
                    .entry(TileId::new(x, run.y))
                    .or_default()
                    .push((index, vec![ring.clone()]));
            }
        }
    }
    Ok(tiles)
}

/// A multipolygon feature's pieces in one tile, in polygon order.
///
/// # Errors
///
/// The first error the slicer yields.
pub fn tile_polygons(
    slicer: &mut Slicer<Coord<i32>>,
    polygons: &[FixturePolygon],
    tile: TileId,
) -> Result<Vec<Rings>, TileError> {
    let mut out = Vec::new();
    for polygon in polygons {
        let input = rings(polygon);
        if let Some(piece) = slicer.tile_polygon(tile, &input)? {
            out.push(piece.rings().map(|r| r.to_vec()).collect());
        }
    }
    Ok(out)
}

/// Tile-insertion orders to try: every permutation for a set of up to `exhaustive` tiles, else a
/// deterministic spread - identity, reverse, and every rotation - for larger sets where `n!` is
/// impractical.
#[must_use]
pub fn orders(n: usize, exhaustive: usize) -> Vec<Vec<usize>> {
    if n <= exhaustive {
        return permutations(n);
    }
    let base: Vec<usize> = (0..n).collect();
    let mut out = vec![base.clone()];
    let mut rev = base;
    rev.reverse();
    out.push(rev);
    for r in 1..n {
        out.push((0..n).map(|i| (i + r) % n).collect());
    }
    out
}

/// A config with the given extent/buffer.
#[must_use]
pub fn slicer(extent: u32, buffer: u16) -> Cfg {
    Cfg { extent, buffer }
}

/// Every permutation of `0..n` - used to prove tile-insertion order independence over a small tile
/// set. Asserts `n <= 10`, so an unexpectedly large set can't explode into `n!` permutations.
#[must_use]
pub fn permutations(n: usize) -> Vec<Vec<usize>> {
    assert!(n <= 10, "permutations of {n} tiles is too many");
    fn go(a: &mut Vec<usize>, k: usize, out: &mut Vec<Vec<usize>>) {
        if k == a.len() {
            out.push(a.clone());
            return;
        }
        for i in k..a.len() {
            a.swap(k, i);
            go(a, k + 1, out);
            a.swap(k, i);
        }
    }
    let mut idx: Vec<usize> = (0..n).collect();
    let mut out = Vec::new();
    go(&mut idx, 0, &mut out);
    out
}

/// Tile extent for the small fixtures (matches the `tests/polylines/fixtures/grid.geojson` grid).
#[must_use]
pub fn grid() -> Cfg {
    slicer(EXTENT, 0)
}

/// The grid config with a 5-unit buffer.
#[must_use]
pub fn grid_buffered() -> Cfg {
    slicer(EXTENT, 5)
}

/// Slicing [`big_polyline`] with each of these yields a different number of output tiles, so the
/// same large geometry can be benchmarked/profiled across output scales (shared by the benchmarks
/// and the `profile` example so both agree). The big polyline spans roughly `[0,420] × [0,535]`:
/// - `multi` (extent 25) -> hundreds of tiles;
/// - `few` (extent 300) -> a 2×2 grid of 4 tiles;
/// - `single` (extent 1024) -> the whole geometry in one tile.
#[must_use]
pub fn big_configs() -> [(&'static str, Cfg); 3] {
    [
        ("multi", slicer(EXTENT, 0)),
        ("few", slicer(300, 0)),
        ("single", slicer(1024, 0)),
    ]
}

/// A set of independent polylines - the fixture representation. Each is added as its own feature;
/// this replaces the old single-`Geometry` (possibly `MultiLineString`) input.
pub type Polylines = Vec<Vec<Coord<i32>>>;

/// The component polylines (vertex slices) of a polyline geometry.
pub fn lines_of(geom: &Geometry<i32>) -> Vec<&[Coord<i32>]> {
    match geom {
        Geometry::LineString(ls) => vec![ls.0.as_slice()],
        Geometry::MultiLineString(mls) => mls.0.iter().map(|ls| ls.0.as_slice()).collect(),
        other => panic!("expected a polyline geometry, got {other:?}"),
    }
}

/// The component polylines of a geometry, owned.
#[must_use]
pub fn polylines_of(geom: &Geometry<i32>) -> Polylines {
    lines_of(geom).into_iter().map(<[_]>::to_vec).collect()
}

/// Slice a set of polylines into per-tile runs, grouped by tile (ordered by [`TileId`]). Each polyline
/// is one part, and within a tile runs come part by part, then in walk order - the order the slicer
/// yields them in. Each run is a plain polyline - runs are never assembled into a `MultiLineString`.
pub fn slice_all_runs(
    cfg: &Cfg,
    polylines: &[Vec<Coord<i32>>],
) -> Vec<(TileId, Vec<Vec<Coord<i32>>>)> {
    let slicer = cfg.slicer();
    group_runs(
        polylines
            .iter()
            .flat_map(|part| slicer.line(part).expect("slice")),
    )
}

/// Group line slices by tile, keeping each tile's runs in yield order.
pub fn group_runs<'a>(
    slices: impl IntoIterator<Item = map_tile_toolkit::LineSlice<'a, Coord<i32>>>,
) -> Vec<(TileId, Vec<Vec<Coord<i32>>>)> {
    let mut tiles: BTreeMap<TileId, Vec<Vec<Coord<i32>>>> = BTreeMap::new();
    for slice in slices {
        tiles
            .entry(slice.tile())
            .or_default()
            .push(slice.run().to_vec());
    }
    tiles.into_iter().collect()
}

/// Clip a set of polylines to one tile -> its runs (empty if nothing lands there), part by part.
pub fn slice_tile_runs(
    cfg: &Cfg,
    polylines: &[Vec<Coord<i32>>],
    tile: TileId,
) -> Vec<Vec<Coord<i32>>> {
    let slicer = cfg.slicer();
    polylines
        .iter()
        .flat_map(|part| slicer.tile_line(tile, part).expect("slice"))
        .map(|s| s.run().to_vec())
        .collect()
}

/// One parsed fixture feature: its `LineString` as integer coordinates plus its GeoJSON properties.
pub struct TestFeature {
    pub line: Vec<Coord<i32>>,
    pub properties: JsonObject,
}

impl TestFeature {}

/// Parse a fixture file into its features: each a `LineString` (whole-number coordinates, truncated
/// to `i32`) and its properties. Fixtures are `FeatureCollection`s; `MultiLineString` is intentionally
/// rejected - express several polylines as several features instead. This is the shared parse/convert
/// core; [`load_fixture_geoms`] keeps only the geometry, other callers also read properties (e.g. a tile id).
pub fn load_fixture(path: &Path) -> Vec<TestFeature> {
    let text = fs::read_to_string(path).expect("readable fixture");
    let GeoJson::FeatureCollection(fc) = text.parse().expect("valid GeoJSON") else {
        panic!("fixture must be a FeatureCollection: {}", path.display());
    };
    let features: Vec<TestFeature> = fc
        .features
        .into_iter()
        .map(|f| {
            let geom = Geometry::<f64>::try_from(f.geometry.expect("feature has geometry"))
                .expect("geometry converts");
            let line = match to_i32(&geom) {
                Geometry::LineString(ls) => ls.0,
                other => panic!(
                    "fixtures must use LineString features, not {other:?} ({}): express multiple \
                     polylines as multiple features",
                    path.display()
                ),
            };
            TestFeature {
                line,
                properties: f.properties.unwrap_or_default(),
            }
        })
        .collect();
    assert!(
        !features.is_empty(),
        "fixture has no features: {}",
        path.display()
    );
    features
}

/// Parse a fixture file into its (integer) polylines - [`load_fixture`] with the properties dropped.
/// Each `LineString` feature is an independent polyline.
pub fn load_fixture_geoms(path: &Path) -> Polylines {
    load_fixture(path).into_iter().map(|f| f.line).collect()
}

/// Every `tests/polylines/fixtures/*.geojson` as `(name, polylines)`, sorted by name for stable ordering.
pub fn load_all_fixtures() -> Vec<(String, Polylines)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/polylines/fixtures");
    let mut out: Vec<(String, Polylines)> = fs::read_dir(&dir)
        .expect("fixtures dir exists")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "geojson"))
        .map(|p| {
            let name = p
                .file_stem()
                .expect("stem")
                .to_str()
                .expect("utf8")
                .to_owned();
            (name, load_fixture_geoms(&p))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(!out.is_empty(), "no fixtures found in {}", dir.display());
    out
}

/// A large, deterministic snake-shaped polyline for benchmarking and large-input correctness
/// checks. It sweeps back and forth (boustrophedon) filling a wide area, so it has many vertices
/// **and** touches many tiles - the case where re-clipping the whole geometry once per tile
/// (`O(vertices × tiles)`) diverges sharply from a single routing pass. Small per-step jitter keeps
/// rows off the axis so segments cross tile boundaries at varied angles. ~3.6k vertices spanning
/// roughly a 420×540 area (≈17×22 tiles on a 25-unit grid).
#[must_use]
pub fn big_polyline() -> Geometry<i32> {
    const ROWS: i32 = 60;
    const COLS: i32 = 60;
    const STEP: i32 = 7; // horizontal vertex spacing (< a 25-unit tile, so segments stay short)
    const ROW_H: i32 = 9; // vertical spacing between rows

    let mut coords = Vec::with_capacity(((ROWS * (COLS + 1)) + 1) as usize);
    for r in 0..ROWS {
        let y0 = r * ROW_H;
        for k in 0..=COLS {
            // Even rows sweep left->right, odd rows right->left, so the path stays connected.
            let x = if r % 2 == 0 {
                k * STEP
            } else {
                (COLS - k) * STEP
            };
            let y = y0 + (k * 3) % 5; // jitter in [0, 4]
            coords.push(Coord { x, y });
        }
    }
    Geometry::LineString(LineString(coords))
}

/// A large, deterministic polygon for benchmarking: a jagged blob of ~3.6k vertices (radius
/// 150-200 around `(220, 270)`, so roughly the [`big_polyline`] footprint) with a ~900-vertex hole
/// - many vertices, many edge tiles, and a wide fully covered interior.
#[must_use]
pub fn big_polygon() -> FixturePolygon {
    let blob = |n: i32, (cx, cy): (f64, f64), r: f64, jitter: f64| -> Vec<Coord<i32>> {
        (0..n)
            .map(|k| {
                let angle = TAU * f64::from(k) / f64::from(n);
                let radius = r + jitter * f64::from((k * 37) % 50) / 50.0;
                Coord {
                    x: (cx + radius * angle.cos()).round() as i32,
                    y: (cy + radius * angle.sin()).round() as i32,
                }
            })
            .collect()
    };
    let mut hole = blob(900, (240.0, 250.0), 40.0, 12.0);
    hole.reverse(); // holes wind opposite to the exterior
    FixturePolygon {
        exterior: blob(3600, (220.0, 270.0), 150.0, 50.0),
        holes: vec![hole],
    }
}

/// A z14-like "ocean": a square covering 16384 × 16384 tiles of extent 4096 (2^28 tiles) with a
/// 21 × 21-tile hole, so nearly every tile is fully covered.
#[must_use]
pub fn huge_square() -> FixturePolygon {
    const N: i32 = 1 << 14;
    const E: i32 = 4096;
    let ring = |lo: i32, hi: i32| {
        vec![
            Coord { x: lo, y: lo },
            Coord { x: hi, y: lo },
            Coord { x: hi, y: hi },
            Coord { x: lo, y: hi },
        ]
    };
    let mut hole = ring((N / 2 - 10) * E + 2000, (N / 2 + 10) * E + 2000);
    hole.reverse();
    FixturePolygon {
        exterior: ring(100, N * E - 100),
        holes: vec![hole],
    }
}

/// A 100×100-tile square (extent 256) with 2000 small holes, one per tile: the shape of a forest or
/// lake multipolygon with many inner rings, whose cost must follow its hits, not its ring count.
#[must_use]
pub fn many_holes() -> FixturePolygon {
    const E: i32 = 256;
    let square = |x: i32, y: i32, side: i32| {
        vec![
            Coord { x, y },
            Coord { x, y: y + side },
            Coord {
                x: x + side,
                y: y + side,
            },
            Coord { x: x + side, y },
        ]
    };
    FixturePolygon {
        exterior: square(0, 0, 100 * E),
        holes: (0..2000)
            .map(|i| square((i % 98 + 1) * E + 10, (i / 98 + 1) * E + 10, 20))
            .collect(),
    }
}

/// A z14-like block of buildings, the bulk of real polygon data: 2000 four-corner footprints, 10-90
/// units a side and turned up to ±0.3 rad, scattered over 4 × 4 tiles of extent 4096. Most lie inside
/// one tile; a few percent cross a tile edge or reach a neighbor's buffer.
#[must_use]
pub fn buildings() -> Vec<FixturePolygon> {
    // A fixed linear congruential sequence, so the benchmark input never changes.
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move |n: u32| -> f64 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        f64::from((state >> 33) as u32 % n)
    };
    (0..2000)
        .map(|_| {
            let (cx, cy) = (next(4 * 4096), next(4 * 4096));
            let (hw, hh) = (5.0 + next(40), 5.0 + next(40));
            let angle = (next(601) - 300.0) / 1000.0;
            let (sin, cos) = angle.sin_cos();
            let corner = |dx: f64, dy: f64| Coord {
                x: (cx + dx * cos - dy * sin).round() as i32,
                y: (cy + dx * sin + dy * cos).round() as i32,
            };
            FixturePolygon {
                exterior: vec![
                    corner(-hw, -hh),
                    corner(hw, -hh),
                    corner(hw, hh),
                    corner(-hw, hh),
                ],
                holes: vec![],
            }
        })
        .collect()
}

/// Building footprints as OSM has them: 2000 closed rings (the first vertex repeated) of 4 to 8
/// distinct vertices, drawn with the shares of OSM Europe's polygons of those sizes (4: 62%, 5: 9%,
/// 6: 14%, 7: 5%, 8: 9% - together 78% of all its polygons), 10-90 units across, turned anyhow, over
/// the same `4 × 4` tiles of extent 4096 as [`buildings`]. Each is a ring around its center at evenly
/// spread angles with jittered radii, so simple; the odd sizes are a little concave.
#[must_use]
pub fn footprints() -> Vec<FixturePolygon> {
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = move |n: u32| -> f64 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        f64::from((state >> 33) as u32 % n)
    };
    (0..2000)
        .map(|_| {
            let pick = next(100);
            let n = match pick as u32 {
                0..62 => 4,
                62..71 => 5,
                71..85 => 6,
                85..91 => 7,
                _ => 8,
            };
            let (cx, cy) = (next(4 * 4096), next(4 * 4096));
            let r = 5.0 + next(40);
            let turn = next(6283) / 1000.0;
            let mut exterior: Vec<Coord<i32>> = (0..n)
                .map(|i| {
                    let a = turn + f64::from(i) * TAU / f64::from(n);
                    let rr = r * (0.6 + next(41) / 100.0);
                    Coord {
                        x: (cx + rr * a.cos()).round() as i32,
                        y: (cy + rr * a.sin()).round() as i32,
                    }
                })
                .collect();
            exterior.dedup();
            exterior.push(exterior[0]);
            FixturePolygon {
                exterior,
                holes: vec![],
            }
        })
        .collect()
}

/// A z14-like block of points on `4 × 4` tiles of extent 4096, from a fixed linear congruential
/// sequence: `n` points, scattered uniformly (`edge_heavy == false`: a few percent land within a
/// buffer of 64 of a tile edge) or all packed into the buffer bands along the tile edges
/// (`edge_heavy`: every point reaches two to four tiles).
#[must_use]
pub fn scattered_points(n: usize, edge_heavy: bool) -> Vec<Coord<i32>> {
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move |n: u32| -> i32 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        ((state >> 33) as u32 % n) as i32
    };
    (0..n)
        .map(|_| {
            if edge_heavy {
                let band = |v: &mut dyn FnMut(u32) -> i32| {
                    v(4) * 4096 + [v(64), 4095 - v(64)][v(2) as usize]
                };
                Coord {
                    x: band(&mut next),
                    y: band(&mut next),
                }
            } else {
                Coord {
                    x: next(4 * 4096),
                    y: next(4 * 4096),
                }
            }
        })
        .collect()
}

/// Tile side of the [`long_diagonal`] / [`diagonal_triangle`] scenes (a typical MVT extent).
pub const DIAGONAL_EXTENT: i32 = 4096;

/// One segment running diagonally across `tiles × tiles` tiles of extent [`DIAGONAL_EXTENT`] - at
/// `tiles = 16384` a z14 world-spanning edge. Slightly off 45° and off the tile corners, so it
/// crosses tile edges at varied positions and passes near (but not through) the corners.
#[must_use]
pub fn long_diagonal(tiles: i32) -> Vec<Coord<i32>> {
    let span = tiles * DIAGONAL_EXTENT;
    vec![
        Coord { x: 100, y: 300 },
        Coord {
            x: span - 200,
            y: span - 1700,
        },
    ]
}

/// A right triangle over `tiles × tiles` tiles whose hypotenuse is the [`long_diagonal`] segment;
/// its other two edges run along a tile row and column, and the half below the diagonal is filled.
#[must_use]
pub fn diagonal_triangle(tiles: i32) -> FixturePolygon {
    let [a, c] = long_diagonal(tiles)[..] else {
        unreachable!("a long diagonal has two vertices")
    };
    FixturePolygon {
        exterior: vec![a, Coord { x: c.x, y: a.y }, c],
        holes: Vec::new(),
    }
}

/// The rings of a polygon, exterior first - the per-polygon input [`Slicer::polygon`] takes.
#[must_use]
pub fn rings(p: &FixturePolygon) -> Vec<&[Coord<i32>]> {
    std::iter::once(p.exterior.as_slice())
        .chain(p.holes.iter().map(Vec::as_slice))
        .collect()
}

/// Convert a polyline geometry to integer coordinates (fixtures use whole numbers).
fn to_i32(geom: &Geometry<f64>) -> Geometry<i32> {
    let ls = |ls: &LineString<f64>| {
        LineString(
            ls.0.iter()
                .map(|c| Coord {
                    x: c.x as i32,
                    y: c.y as i32,
                })
                .collect(),
        )
    };
    match geom {
        Geometry::LineString(l) => Geometry::LineString(ls(l)),
        Geometry::MultiLineString(m) => {
            Geometry::MultiLineString(MultiLineString(m.0.iter().map(ls).collect()))
        }
        other => panic!("expected a polyline geometry, got {other:?}"),
    }
}

/// Convert an integer polyline geometry to `f64` for GeoJSON output. Inverse of [`to_i32`].
pub fn to_f64(geom: &Geometry<i32>) -> Geometry<f64> {
    let ls = |ls: &LineString<i32>| {
        LineString(
            ls.0.iter()
                .map(|c| Coord {
                    x: f64::from(c.x),
                    y: f64::from(c.y),
                })
                .collect(),
        )
    };
    match geom {
        Geometry::LineString(l) => Geometry::LineString(ls(l)),
        Geometry::MultiLineString(m) => {
            Geometry::MultiLineString(MultiLineString(m.0.iter().map(ls).collect()))
        }
        other => panic!("expected a polyline geometry, got {other:?}"),
    }
}

/// A GeoJSON [`Feature`] wrapping `geom` with the given [simplestyle-spec] properties. Because a
/// snapshot file ends in `.geojson`, GitHub and geojson.io render the properties (`stroke`/`fill`/
/// …) directly on a map.
///
/// [simplestyle-spec]: https://github.com/mapbox/simplestyle-spec
pub fn feature(geom: &Geometry<f64>, props: Vec<(&str, JsonValue)>) -> Feature {
    let properties = props
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect::<JsonObject>();
    Feature {
        bbox: None,
        geometry: Some(geojson::Geometry::new(GeometryValue::from(geom))),
        id: None,
        properties: Some(properties),
        foreign_members: None,
    }
}

/// A GeoJSON `LineString` [`Feature`] for one integer run (converted to `f64`) with the given
/// [`simplestyle-spec`](https://github.com/mapbox/simplestyle-spec) properties.
pub fn line_feature(run: &[Coord<i32>], props: Vec<(&str, JsonValue)>) -> Feature {
    feature(
        &to_f64(&Geometry::LineString(LineString(run.to_vec()))),
        props,
    )
}

/// A `LineString` feature for one run tagged with the simplestyle `role`, `stroke` color, and
/// `stroke-width` - the shape every snapshot feature uses.
fn styled_line(run: &[Coord<i32>], role: &str, stroke: &str, width: u32) -> Feature {
    line_feature(
        run,
        vec![
            ("role", json!(role)),
            ("stroke", json!(stroke)),
            ("stroke-width", json!(width)),
        ],
    )
}

pub fn feature_line(run: &[Coord<i32>], role: &str) -> Feature {
    styled_line(run, role, "#261fb5", 2)
}

pub fn input_feature(run: &[Coord<i32>]) -> Feature {
    styled_line(run, "input", "#f6fd31", 15)
}

// ---- Polygon fixtures & rendering (shared by the polygon snapshot tests) ----

/// One polygon fixture feature: an exterior ring plus zero or more interior rings (holes), in integer
/// coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixturePolygon {
    pub exterior: Vec<Coord<i32>>,
    pub holes: Vec<Vec<Coord<i32>>>,
}

/// A ring's integer coordinates (whole-number lon/lat truncated to `i32`).
fn ring_i32(ls: &LineString<f64>) -> Vec<Coord<i32>> {
    ls.0.iter()
        .map(|c| Coord {
            x: c.x as i32,
            y: c.y as i32,
        })
        .collect()
}

/// Parse a polygon fixture into its features: a `FeatureCollection` of `Polygon` or `MultiPolygon`
/// features, each yielding its polygons (an exterior ring plus optional interior rings / holes).
pub fn load_polygon_features(path: &Path) -> Vec<Vec<FixturePolygon>> {
    let text = fs::read_to_string(path).expect("readable fixture");
    let GeoJson::FeatureCollection(fc) = text.parse().expect("valid GeoJSON") else {
        panic!(
            "polygon fixture must be a FeatureCollection: {}",
            path.display()
        );
    };
    let polygon = |p: &Polygon<f64>| FixturePolygon {
        exterior: ring_i32(p.exterior()),
        holes: p.interiors().iter().map(ring_i32).collect(),
    };
    let features: Vec<Vec<FixturePolygon>> = fc
        .features
        .into_iter()
        .map(|f| {
            let geom = Geometry::<f64>::try_from(f.geometry.expect("feature has geometry"))
                .expect("geometry converts");
            match geom {
                Geometry::Polygon(p) => vec![polygon(&p)],
                Geometry::MultiPolygon(mp) => mp.0.iter().map(polygon).collect(),
                other => panic!(
                    "polygon fixtures must use Polygon or MultiPolygon features, not {other:?} ({})",
                    path.display()
                ),
            }
        })
        .collect();
    assert!(
        !features.is_empty(),
        "polygon fixture has no features: {}",
        path.display()
    );
    features
}

/// Every polygon of a fixture, a `MultiPolygon` feature contributing each of its parts - for the
/// single-tile slicer, which takes one polygon per feature.
pub fn load_polygon_fixture(path: &Path) -> Vec<FixturePolygon> {
    load_polygon_features(path).into_iter().flatten().collect()
}

/// Inclusive tile-coordinate bounds covering every vertex of `rings`, padded by one tile so a per-tile
/// scan also visits the empty tiles just outside the geometry. Works for polyline or polygon rings.
#[must_use]
pub fn padded_tile_span(rings: &[Vec<Coord<i32>>]) -> (TileId, TileId) {
    let mut lo = TileId::new(i32::MAX, i32::MAX);
    let mut hi = TileId::new(i32::MIN, i32::MIN);
    let e = EXTENT as i32;
    for ring in rings {
        for &c in ring {
            let (tx, ty) = (c.x.div_euclid(e), c.y.div_euclid(e));
            lo = TileId::new(lo.x.min(tx), lo.y.min(ty));
            hi = TileId::new(hi.x.max(tx), hi.y.max(ty));
        }
    }
    (
        TileId::new(lo.x - 1, lo.y - 1),
        TileId::new(hi.x + 1, hi.y + 1),
    )
}

/// A ring's `f64` coordinates for GeoJSON output.
fn ring_f64(ring: &[Coord<i32>]) -> LineString<f64> {
    LineString(
        ring.iter()
            .map(|c| Coord {
                x: f64::from(c.x),
                y: f64::from(c.y),
            })
            .collect(),
    )
}

/// A filled GeoJSON `Polygon` feature (exterior plus `holes`, so holes render punched out), styled so
/// it renders on a map and tagged with `role`.
fn styled_polygon(
    exterior: &[Coord<i32>],
    holes: &[Vec<Coord<i32>>],
    role: &str,
    fill: &str,
    stroke: &str,
) -> Feature {
    let poly = Polygon::new(
        ring_f64(exterior),
        holes.iter().map(|h| ring_f64(h)).collect(),
    );
    feature(
        &Geometry::Polygon(poly),
        vec![
            ("role", json!(role)),
            ("fill", json!(fill)),
            ("fill-opacity", json!(0.35)),
            ("stroke", json!(stroke)),
            ("stroke-width", json!(2)),
        ],
    )
}

/// The original input polygon (bright yellow), the reference every per-tile piece should tile back to.
pub fn input_polygon(exterior: &[Coord<i32>], holes: &[Vec<Coord<i32>>]) -> Feature {
    styled_polygon(exterior, holes, "input", "#f6fd31", "#b5a300")
}

/// One tile's clipped polygon piece (exterior + holes), filled and colored by tile parity so neighbors
/// contrast, tagged `tile x/y`.
pub fn tile_polygon(exterior: &[Coord<i32>], holes: &[Vec<Coord<i32>>], tile: TileId) -> Feature {
    let role = format!("tile {}/{}", tile.x, tile.y);
    let fill = if (tile.x + tile.y).rem_euclid(2) == 0 {
        "#261fb5"
    } else {
        "#b5211f"
    };
    styled_polygon(exterior, holes, &role, fill, "#111111")
}

// ---- Point fixtures & rendering (shared by the point snapshot tests) ----

/// Parse a point fixture into its features: a `FeatureCollection` of `Point` or `MultiPoint` features,
/// each yielding its points (whole-number coordinates truncated to `i32`).
pub fn load_point_features(path: &Path) -> Vec<Vec<Coord<i32>>> {
    let text = fs::read_to_string(path).expect("readable fixture");
    let GeoJson::FeatureCollection(fc) = text.parse().expect("valid GeoJSON") else {
        panic!(
            "point fixture must be a FeatureCollection: {}",
            path.display()
        );
    };
    let point = |p: &Point<f64>| Coord {
        x: p.x() as i32,
        y: p.y() as i32,
    };
    let features: Vec<Vec<Coord<i32>>> = fc
        .features
        .into_iter()
        .map(|f| {
            let geom = Geometry::<f64>::try_from(f.geometry.expect("feature has geometry"))
                .expect("geometry converts");
            match geom {
                Geometry::Point(p) => vec![point(&p)],
                Geometry::MultiPoint(mp) => mp.0.iter().map(point).collect(),
                other => panic!(
                    "point fixtures must use Point or MultiPoint features, not {other:?} ({})",
                    path.display()
                ),
            }
        })
        .collect();
    assert!(
        !features.is_empty(),
        "point fixture has no features: {}",
        path.display()
    );
    features
}

/// A GeoJSON `MultiPoint` feature with the given simplestyle marker properties (its `role` among
/// them): all of `points` in one feature.
fn styled_multi_point(points: &[Coord<i32>], props: Vec<(&str, JsonValue)>) -> Feature {
    feature(
        &Geometry::MultiPoint(MultiPoint(
            points
                .iter()
                .map(|c| Point::new(f64::from(c.x), f64::from(c.y)))
                .collect(),
        )),
        props,
    )
}

/// Every original input point in one `MultiPoint` feature (large, bright yellow), the reference every
/// per-tile feature sits on.
pub fn input_points(points: &[Coord<i32>]) -> Feature {
    styled_multi_point(
        points,
        vec![
            ("role", json!("input")),
            ("marker-color", json!("#f6fd31")),
            ("marker-size", json!("large")),
        ],
    )
}

/// One tile's points in one `MultiPoint` feature (global coordinates), colored by tile parity so
/// neighbors contrast, tagged `tile x/y`. `indices` are the positions of the input points they are
/// copies of, in the [`input_points`] feature, in the same order as `points`.
pub fn tile_points(points: &[Coord<i32>], tile: TileId, indices: &[u32]) -> Feature {
    let color = if (tile.x + tile.y).rem_euclid(2) == 0 {
        "#261fb5"
    } else {
        "#b5211f"
    };
    styled_multi_point(
        points,
        vec![
            ("role", json!(format!("tile {}/{}", tile.x, tile.y))),
            ("indices", json!(indices)),
            ("marker-color", json!(color)),
            ("marker-size", json!("small")),
        ],
    )
}

/// Serialize `features` as a pretty-printed GeoJSON `FeatureCollection` - the byte form the snapshot
/// tests store and compare. Features are sorted by their `role` property: inputs, then tiles, then
/// fills, then any other role, each group by role text (stable, so pieces sharing a role keep the
/// order they were produced in).
pub fn feature_collection_bytes(mut features: Vec<Feature>) -> Vec<u8> {
    features.sort_by_cached_key(|f| {
        let role = f
            .property("role")
            .and_then(JsonValue::as_str)
            .unwrap_or_default();
        let group = if role == "input" {
            0
        } else if role.starts_with("tile ") {
            1
        } else if role.starts_with("fill ") {
            2
        } else {
            3
        };
        (group, role.to_owned())
    });
    serde_json::to_vec_pretty(&FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    })
    .expect("serializes")
}

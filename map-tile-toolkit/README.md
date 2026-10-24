# map-tile-toolkit

[![crates.io version](https://img.shields.io/crates/v/map-tile-toolkit)](https://crates.io/crates/map-tile-toolkit)
[![docs.rs status](https://img.shields.io/docsrs/map-tile-toolkit)](https://docs.rs/map-tile-toolkit)
[![crates.io license](https://img.shields.io/crates/l/map-tile-toolkit)](https://github.com/maplibre/martin/blob/main/LICENSE-APACHE)

Slice integer **lines**, **points** and **polygons** into per-tile pieces on a square tile grid, keeping
the geometry's original vertices. A line never gets a new vertex; a polygon ring gets only synthetic
corners outside the tile, to stay closed. Each tile may have a buffer (under half a tile), which widens
the box a tile keeps and the geometry neighboring tiles share. `Mosaic` and `PolygonMosaic` reassemble
the tiles into whole features, exactly: unlike clippers that cut new vertices at the tile edge
(Sutherland-Hodgman, `geo`'s boolean ops), a segment crossing a border is the same original segment in
both tiles.

## Usage

`Slicer` takes **one geometry** at a time and returns a lazy iterator over its slices:

* `line`, `point`, `polygon` - every tile the geometry reaches, even tiles holding none of its vertices;
* `tile_line`, `tile_point`, `tile_polygon` - one given tile, exactly what the first call yields there.

Each slice is computed only when the iterator reaches it, so stopping early stops the work. A line run
borrows the caller's vertices, so a line with millions of vertices costs no extra memory. Slices come in
no particular order, and a line that leaves a tile and comes back gives it several runs. Nothing
panics: bad input yields a `TileError`.

```rust
use geo_types::coord;
use map_tile_toolkit::{Slicer, TileError};

fn main() -> Result<(), TileError> {
    let line = [
        coord!{ x: 5, y: 5 },
        coord!{ x: 20, y: 20 },
        coord!{ x: 60, y: 40 },
    ];

    // `extent = 25` -> 25-unit tiles;
    // `buffer = 0` = tight clip box (must be < extent / 2).
    let slicer = Slicer::new(25, 0)?;

    // One slice per run, each in its tile's local frame.
    for slice in slicer.line(&line)? {
        for vertex in slice.run() {
            let _ = (slice.tile(), vertex); // vertex: Coord<i32>
        }
    }

    Ok(())
}
```

### Lines

A vertex is kept when one of its two segments touches the tile's buffered box, and a run breaks only
where a vertex is dropped. So a run ends at the first vertex past the edge, a segment crossing the tile
keeps both its outside ends, a single segment outside is bridged, and only a longer excursion splits the
line into several runs:

![Which line vertices a tile keeps](https://raw.githubusercontent.com/maplibre/martin/main/map-tile-toolkit/docs/images/clip-lines.svg)

### Polygons

`polygon(&rings)` and `tile_polygon(tile, &rings)` take one polygon as its rings, exterior first. A tile
gets one **piece**: the exterior and the holes reaching the tile, each ring closed (its first vertex
repeated; drop it for an MVT `ClosePath`) and wound like the input.

A ring keeps every vertex with an edge touching the buffered box. Where it leaves the box and comes back,
its outside vertices are compared with the synthetic corners a detour winding around the tile the same
way would need: with more vertices, the detour replaces them (a straight chord when it needs no corner),
otherwise they are kept. A tile the polygon covers gets just the four corners, and a tile inside a hole
gets nothing. Synthetic corners sit one unit outside the buffered box, at tile-local `-buffer - 1` or
`extent + buffer`, so they never land in it.

![Which polygon vertices a tile keeps](https://raw.githubusercontent.com/maplibre/martin/main/map-tile-toolkit/docs/images/clip-polygons.svg)

A piece is a **valid polygon** (OGC: simple rings, holes inside the exterior and clear of each other)
whenever the input is, outside the buffered box too, so a consumer can use a piece on its own,
triangulating it say, without clipping it to the tile. The plain clip above is not always valid out there:
each ring is clipped on its own, so a hole crossing the tile keeps its outside vertices even where the
exterior was cut short, and past the box it can reach outside the exterior, or two detours can cross. A
piece whose plain clip is valid is kept, so most cost one check of their outside segments: those with an
endpoint outside the box, the edges that cross out and the corners and chords that close the rings (a
ring that never leaves the box, or one whose gaps keep their original vertices, has none to check).
Otherwise the piece is rebuilt, trying in turn:

* a tile the exterior covers gets a fill box around the holes' kept vertices, not just around the box;
* every excursion closes on a **frame**, the convex hull of everything kept outside the box grown by a
  lane, each tip leaving the hull through a short stub, and each excursion in a lane of its own; a tip
  keeps as many more original vertices as it takes to be a vertex of the hull;
* each excursion keeps a few more original vertices past its tip, with no frame;
* at worst, every excursion whole, which is valid because the input is.

The first vertex outside the box of every edge crossing its border is always kept, and the rebuilt rings
stay outside the box wherever they differ from the plain clip, so [`PolygonMosaic`] reassembles repaired
pieces exactly as it does plain ones: it keeps only the edges touching each tile's buffered box.

A piece keeps its plain clip, **invalid** outside the box, when its repair would cost too much: when the
checks and probes of the tile run out of their budget (about 65 000 segment pairs, of those whose `x`
ranges overlap), or when only the last resort works and the polygon has over 2048 vertices, rather than
cost the tile its whole ring. So garbage input (a self-intersecting ring, holes that overlap), for which
nothing is promised, still slices in bounded time. The repairs are not cheap: on spiky valid polygons,
where most edge tiles need one, slicing took 6 to 12 times as long as the plain clip alone, and the pieces
kept 40% to 60% more vertices.

The polygon is sliced row by row: the call reads the rings once, to bound and index them, and the
iterator then holds one tile row at a time (a polygon inside one tile is lent as it is). Tiles the
polygon covers entirely come as `FillRun { y, x }` row spans, so cost grows with the perimeter, not the
area. `fill_ring(&run)` is the one ring each of those tiles holds: the four corners, closed. Pieces are
built in the slicer's reused buffer, so the iterator is *lending* (loop with `while let`) and polygons
need `&mut Slicer`. Reuse one slicer per worker: it stops allocating once warmed up, and
`shrink_to_fit()` releases what an outlier left behind.

`signed_area_2x` gives a ring's exact orientation, to normalize winding once per feature. MVT wants
exteriors with a positive area in its y-down tile coordinates, and holes with a negative one.

```rust
use geo_types::coord;
use map_tile_toolkit::{PolygonSlice, Slicer, TileError, signed_area_2x};

fn main() -> Result<(), TileError> {
    let mut exterior = vec![
        coord! { x: 5, y: 5 }, coord! { x: 120, y: 5 },
        coord! { x: 120, y: 120 }, coord! { x: 5, y: 120 },
    ];
    let mut hole = vec![
        coord! { x: 40, y: 40 }, coord! { x: 40, y: 60 },
        coord! { x: 60, y: 60 }, coord! { x: 60, y: 40 },
    ];
    // MVT winding: exterior positive, holes negative.
    if signed_area_2x(&exterior) < 0 { exterior.reverse(); }
    if signed_area_2x(&hole) > 0 { hole.reverse(); }

    let mut slicer = Slicer::new(25, 2)?;
    // A polygon is its rings, exterior first.
    let rings = [exterior, hole];
    let mut slices = slicer.polygon(&rings)?;
    while let Some(slice) = slices.next() {
        match slice? {
            // The polygon in one tile: its rings, exterior first, each closed and tile-local.
            PolygonSlice::Piece(piece) => {
                for ring in piece.rings() {
                    let _ = (piece.tile(), ring.to_vec());
                }
            }
            // Tiles (x, y) for x in run.x, entirely inside the polygon: each holds it as the
            // single ring `slices.fill_ring(&run)`.
            PolygonSlice::Run(run) => {
                let _ = (run.y, &run.x, slices.fill_ring(&run));
            }
        }
    }
    Ok(())
}
```

### Points

`point(p)` yields the point in every tile whose buffered box holds it: its own tile, plus up to three
neighbors when it lies within `buffer` of an edge. The iterator knows its length (one to four).
`tile_point(tile, p)` returns the point in one tile's frame, if the tile holds it.

```rust
use geo_types::coord;
use map_tile_toolkit::{Slicer, TileError, TileId};

fn main() -> Result<(), TileError> {
    let slicer = Slicer::new(256, 8)?;

    // One unit past the corner of tile (1, 1): within the buffer of three neighbors as well.
    let point = coord! { x: 257, y: 256 };
    let slices = slicer.point(point)?;
    assert_eq!(slices.len(), 4);
    for slice in slices {
        let _ = (slice.tile(), slice.point()); // TileId, and the point as Coord<i32> in that tile
    }

    assert_eq!(
        slicer.tile_point(TileId::new(1, 1), point)?,
        Some(coord! { x: 1, y: 0 })
    );
    assert_eq!(slicer.tile_point(TileId::new(5, 5), point)?, None);
    Ok(())
}
```

### Unclipped

A slicer built `with_clip_mode(ClipMode::Unclipped)` gives each tile a line or polygon reaches (exactly
the tiles the default `Clipped` mode gives anything) the **whole** geometry once, in that tile's frame,
for renderers that clip themselves. The calls and items are the same, so one consumer loop serves both
modes: a line comes as one `LineSlice` per tile holding all of it, and a polygon as one `PolygonPiece`
per tile with all its rings, in covered tiles too (no `FillRun`). Slices borrow the input, so the
slicer's cost is finding the tiles; to give a line's tile just once it keeps a set of the tiles found so
far. The consumer pays the whole geometry per tile. Points are unaffected.

```rust
use geo_types::coord;
use map_tile_toolkit::{ClipMode, Slicer, TileError, TileId};

fn main() -> Result<(), TileError> {
    let slicer = Slicer::new(25, 0)?.with_clip_mode(ClipMode::Unclipped);
    let line = [coord! { x: 5, y: 5 }, coord! { x: 60, y: 10 }];

    // Tiles (0, 0), (1, 0) and (2, 0), each holding the whole line in its own frame.
    let mut tiles: Vec<_> = slicer
        .line(&line)?
        .map(|slice| (slice.tile(), slice.run().to_vec()))
        .collect();
    tiles.sort_unstable_by_key(|(tile, _)| *tile);
    assert_eq!(tiles.len(), 3);
    assert_eq!(
        tiles[1],
        (TileId::new(1, 0), vec![coord! { x: -20, y: 5 }, coord! { x: 35, y: 10 }])
    );
    Ok(())
}
```

### Multi-geometries and attributes

The slicer has no multi-geometries and no feature attributes. Slice a `MultiLineString`, `MultiPoint` or
`MultiPolygon` one element at a time, and pair every slice with its feature's id and properties: the
caller does that per tile anyway, and it keeps the slicer free of state that grows with the feature.
Slices of several elements landing in one tile are several slices there, which the caller groups (into
one `MultiLineString` per tile, say). See
[`examples/mvt_features.rs`](https://github.com/maplibre/martin/blob/main/map-tile-toolkit/examples/mvt_features.rs)
for a round trip emitting per-tile `GeoJSON` with ids and properties.

```rust
use geo_types::coord;
use map_tile_toolkit::{Slicer, TileError};

struct Attrs {
    id: Option<u64>,
    name: &'static str
}

fn main() -> Result<(), TileError> {
    let slicer = Slicer::new(25, 0)?;
    let attrs = Attrs { id: Some(1), name: "Main St" };
    let line = [coord! { x: 5, y: 5 }, coord! { x: 60, y: 40 }];
    for slice in slicer.line(&line)? {
        // Every slice of this feature goes with `attrs`, in whichever tile it lands.
        let _ = (slice.tile(), attrs.id, attrs.name);
    }
    Ok(())
}
```

Per-vertex data is the `Vertex` type instead (default `Coord<i32>`): `Measured<M>` pairs a position with
any `Copy + PartialEq` payload (an M value, an id), which rides through slicing and merging unchanged.
A polygon's synthetic corners carry `M::default()`.

```rust
use map_tile_toolkit::{Measured, Slicer, TileError};

fn main() -> Result<(), TileError> {
    // Each vertex carries a payload (here an id); it survives slicing untouched.
    let slicer = Slicer::new(25, 0)?;
    let line = [
        Measured::new(5, 5, 100),
        Measured::new(20, 20, 200),
        Measured::new(60, 40, 300),
    ];
    for slice in slicer.line(&line)? {
        let _ = slice.run().iter().map(|v| (v.position, v.m)).collect::<Vec<_>>();
    }
    Ok(())
}
```

### Coordinate space

The slicer is integer-only: `extent` is both the tile side and its output resolution (the vector-tile
`extent`). Vertex `x` belongs to tile `x.div_euclid(extent)`, whose own area is `[0, extent)` in its
local frame `x - tile · extent`; buffer vertices, a line's first vertex past an edge and synthetic
corners lie outside it. Add `tile · extent` to get global coordinates back. Do float and projection work
up front (e.g. with [`geo`](https://docs.rs/geo)); for web mercator at zoom `z`, project, simplify, then
scale by `2^z · extent / circumference`, move the top-left corner to the origin, flip `y`, and round,
landing in `[0, 2^z · extent)`.

### Rounding polygons

Rounding a polygon's vertices one by one can make a valid polygon invalid: two edges closer than a unit
can end up crossing, touching, or folding onto each other. `Quantizer::polygon` rounds one valid float
polygon, already scaled to grid units, and gives zero or more valid integer polygons. It rounds halves to
even, as `PostGIS` does, so `0.5` and `-0.5` round to `0` and `1.5` to `2`. If the rounded rings
are still valid, they are what it gives. Otherwise it snap-rounds the polygon: each edge bends through
every grid point whose unit square it crosses, if a vertex or a crossing of two edges rounds to that
point, so no two rounded edges cross. Then it rebuilds the area as valid polygons: two tip vertices merge
into one, a hole becomes a notch where it rounds onto the exterior, a fold that collapsed is dropped, and
a narrow neck that collapsed to a point splits the polygon in two. Rings come open, exteriors with a
positive `signed_area_2x` and holes with a negative one.

Rounding moves a point less than a unit along each axis, so `polygon` only checks the edges that come
within a unit of a vertex. That relies on the polygon being valid: an invalid one may come out as invalid
as it went in, and debug builds panic when it does. `Quantizer::make_valid` takes any polygon and checks
every pair of rounded edges. Its area is what the exterior covers, minus what any hole covers. So both
lobes of a self-crossing exterior are kept, split at an added vertex where its edges cross, and a hole
only takes out its part inside the exterior.

Rounding the parts of a multipolygon one by one can make them touch or overlap: two parts under a unit
apart round onto one edge. `Quantizer::multi_polygon` takes polygons that are each valid but may touch or
overlap, as independently simplified parts can. A part whose rounded vertices stay clear of every other
part's is rounded as `polygon` rounds it. Parts that come closer are rounded together, kept if none of
their rounded edges meet, and otherwise merged into their union, as `ST_ReducePrecision` does in `PostGIS`.

In the example below, the tip's two vertices are under a unit apart, so rounded one by one its edges
would cross: instead the two tip vertices merge into one.

![How Quantizer rounds polygons](https://raw.githubusercontent.com/maplibre/martin/main/map-tile-toolkit/docs/images/quantize.svg)

```rust
use geo_types::coord;
use map_tile_toolkit::{Quantizer, TileError};

fn main() -> Result<(), TileError> {
    let mut quantizer = Quantizer::new();
    let exterior = [
        coord! { x: 1612.28, y: 395.41 }, coord! { x: 1617.38, y: 388.41 },
        coord! { x: 1654.28, y: 308.21 }, coord! { x: 1653.54, y: 308.55 },
        coord! { x: 1618.68, y: 323.32 }, coord! { x: 1609.48, y: 368.51 },
        coord! { x: 1607.98, y: 394.02 },
    ];
    for polygon in quantizer.polygon([exterior])?.polygons() {
        for ring in polygon {
            assert_eq!(ring.len(), 6);
        }
    }
    Ok(())
}
```

### Limits

A segment is walked through exactly the tiles it touches, so cost follows those tiles, not its bounding
box: a world-spanning diagonal at z14 (extent 4096) touches about 35,000 tiles. Each call bounds the
geometry first and returns any error then, so the iterators cannot fail midway (except as noted):

* **`i32` coordinates** (`Overflow`): `2^z · extent` must fit (up to z18 at extent 4096), and so must
  the buffered box of every tile the geometry reaches. A line or polygon spanning more than `2^31` units
  on one axis is rejected whole.
* **`u32` indexing** (`GeometryTooLarge`): at most `u32::MAX` rings or edges per polygon, and `u32::MAX`
  vertices per piece; the last is the one error a polygon iterator can yield.

There is no limit on how many tiles a geometry reaches: memory stays bounded, and the work is only what
the caller pulls.

### Merging tiles back

`Mosaic` is the inverse of `Slicer::line`: add each tile's runs, in its local frame, and it reassembles
whole features across tile borders, in the global frame. Not every tile is needed. It rejects a tile
inconsistent with those already added (a shared segment that differs, or a tile that owns a vertex but
lacks its segment), names the conflicting tiles, and stays unchanged. `purge` drops a tile, and
`iter_features` walks the features. `PolygonMosaic` does the same for polygon pieces: it keeps only the
ring edges touching each tile's buffered box (so it needs the `buffer` too), which drops every synthetic
edge, since those lie strictly outside it.

```rust
use std::collections::BTreeMap;

use geo_types::coord;
use map_tile_toolkit::{Mosaic, Slicer, TileError};

fn main() -> Result<(), TileError> {
    let slicer = Slicer::new(25, 0)?;
    let line = [coord!{ x: 5, y: 5 }, coord!{ x: 60, y: 40 }];

    // Gather each tile's runs, then hand each tile to the mosaic.
    let mut tiles = BTreeMap::<_, Vec<_>>::new();
    for slice in slicer.line(&line)? {
        tiles.entry(slice.tile()).or_default().push(slice.run().to_vec());
    }
    let mut mosaic = Mosaic::new(slicer.extent())?;
    for (tile, runs) in &tiles {
        mosaic.add(*tile, runs).expect("consistent tiles never conflict");
    }
    for feature in mosaic.iter_features() {
        let _ = feature; // Vec<Coord<i32>> in global coordinates
    }
    Ok(())
}
```

## Development

This crate lives in the [Martin](https://github.com/maplibre/martin) workspace, whose
[just](https://github.com/casey/just#readme) module `just toolkit` lists its commands: `just toolkit::test`
runs the tests, and `just toolkit::bless` regenerates the snapshots. Tests are data-driven: every fixture in
`tests/{polylines,polygons,points}/fixtures/` is sliced by both the all-tiles and the one-tile calls,
which must agree, also clipped and unclipped. The results are snapshotted as `GeoJSON` that renders on a
map; `just toolkit::visualize-tests` builds `tests/visualize.qgz` to inspect them all in QGIS, over the tile grid
in `tests/grid.geojson`.

## License

Licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](../LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
* MIT license ([LICENSE-MIT](../LICENSE-MIT) or <https://opensource.org/licenses/MIT>)
  at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual-licensed as above, without any
additional terms or conditions.

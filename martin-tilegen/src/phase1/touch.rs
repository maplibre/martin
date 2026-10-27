//! What `ST_AsMVTGeom` keeps when it clips: a line or polygon that meets a tile's buffered box only on its
//! edge is dropped there, as its intersection with the box is of lower dimension than the geometry.

use std::ops::RangeInclusive;

use geo::Relate as _;
use geo::coordinate_position::CoordPos;
use geo::dimensions::Dimensions;
use geo_types::{Coord, LineString, Polygon, Rect};

use crate::sort::record::Vertex;

/// A tile's closed buffered box: the same span on both axes of its frame.
#[derive(Clone, Debug)]
pub(crate) struct TileBox {
    span: RangeInclusive<i32>,
}

impl TileBox {
    /// The box `[-buffer, extent + buffer]` of a tile with this `extent` and `buffer`.
    pub(crate) fn new(extent: i32, buffer: i32) -> Self {
        Self {
            span: -buffer..=extent + buffer,
        }
    }

    /// Whether the tile-local `run` lies in the box along a length, not only at points.
    pub(crate) fn holds_length(&self, run: &[Vertex]) -> bool {
        if run.iter().any(|&v| self.surrounds(v)) {
            return true;
        }
        let matrix = line(run).relate(&self.rect());
        [CoordPos::Inside, CoordPos::OnBoundary]
            .into_iter()
            .any(|on| matrix.get(CoordPos::Inside, on) == Dimensions::OneDimensional)
    }

    /// Whether the polygon of the open, tile-local `rings` (exterior first) covers some area of the box.
    pub(crate) fn holds_area<'a>(&self, rings: impl IntoIterator<Item = &'a [Vertex]>) -> bool {
        let rings: Vec<&[Vertex]> = rings.into_iter().collect();
        let Some((exterior, holes)) = rings.split_first() else {
            return false;
        };
        if rings.iter().copied().flatten().any(|&v| self.surrounds(v)) {
            return true;
        }
        let polygon = Polygon::new(line(exterior), holes.iter().map(|hole| line(hole)).collect());
        polygon.relate(&self.rect()).get(CoordPos::Inside, CoordPos::Inside) == Dimensions::TwoDimensional
    }

    /// Whether `v` lies inside the box, off its edge, so its geometry passes through the box's interior:
    /// the common case, settled without relating whole geometries.
    fn surrounds(&self, v: Vertex) -> bool {
        let inside = |c: i32| *self.span.start() < c && c < *self.span.end();
        inside(v[0]) && inside(v[1])
    }

    /// The box as a `geo` rectangle.
    fn rect(&self) -> Rect<f64> {
        let (lo, hi) = (f64::from(*self.span.start()), f64::from(*self.span.end()));
        Rect::new(Coord { x: lo, y: lo }, Coord { x: hi, y: hi })
    }
}

/// The tile-local `vertices` as a `geo` line.
fn line(vertices: &[Vertex]) -> LineString<f64> {
    vertices
        .iter()
        .map(|&[x, y]| Coord {
            x: f64::from(x),
            y: f64::from(y),
        })
        .collect()
}

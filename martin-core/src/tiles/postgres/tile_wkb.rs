//! Reader for `PostGIS` WKB/EWKB geometries that already live in MVT tile coordinate space.
//!
//! `ST_AsMVTGeom` returns geometry in tile space but keeps the M ordinate of points and lines,
//! while `ST_AsMVT` discards it. Serving MLT directly from `PostgreSQL` therefore skips `ST_AsMVT` and reads the
//! output of `ST_AsBinary(ST_AsMVTGeom(...))` here instead.

use geo_traits::{
    CoordTrait, Dimensions, GeometryTrait as _, GeometryType, LineStringTrait,
    MultiLineStringTrait as _, MultiPointTrait as _, MultiPolygonTrait as _, PointTrait,
    PolygonTrait,
};
use mlt_core::geo_types::{
    Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
};
use wkb::reader::Wkb;

/// Errors that can occur while reading tile-space WKB.
#[non_exhaustive]
#[derive(thiserror::Error, Debug, Clone, PartialEq)]
pub enum TileWkbError {
    /// The buffer does not hold a readable WKB geometry.
    #[error("Unreadable WKB: {0}")]
    Unreadable(String),

    /// The geometry ended before the buffer did.
    #[error("{0} trailing bytes after the end of the WKB geometry")]
    TrailingBytes(usize),

    /// A coordinate is not a tile coordinate: not finite, or outside [`i32`].
    #[error("Coordinate {0} does not fit into an i32 tile coordinate")]
    CoordinateOutOfRange(f64),

    /// A `POINT EMPTY` has no coordinate to place in a tile.
    #[error("An empty WKB point is not a tile geometry")]
    EmptyPoint,

    /// A measured geometry has a vertex without an M ordinate.
    #[error("A measured WKB geometry has a vertex without an M ordinate")]
    MissingMeasure,

    /// `ST_AsMVTGeom` never returns this geometry, so neither does this reader.
    #[error("A WKB {0} is not a tile geometry")]
    UnsupportedGeometry(&'static str),
}

/// Reads one `PostGIS` WKB or EWKB geometry whose coordinates are already in tile space.
///
/// Z ordinates are discarded. The M ordinates, when the geometry has them, come back as a second
/// value holding one entry per vertex of the returned geometry, in the order `geo_types` stores
/// them. The whole buffer must be consumed by exactly one geometry.
///
/// A single-part multi-geometry collapses to its singular form, the way MVT encodes it.
pub fn parse_tile_wkb(bytes: &[u8]) -> Result<(Geometry<i32>, Option<Vec<f64>>), TileWkbError> {
    let wkb = Wkb::try_new(bytes).map_err(|e| TileWkbError::Unreadable(e.to_string()))?;
    let trailing = bytes.len() - wkb.buf().len();
    if trailing > 0 {
        return Err(TileWkbError::TrailingBytes(trailing));
    }
    let mut reader = Reader::new(wkb.dim());
    let geometry = reader.geometry(&wkb)?;
    Ok((geometry, reader.finish()))
}

/// Walks one geometry, rounding its vertices into tile space and collecting their M ordinates.
struct Reader {
    m_index: Option<usize>,
    m_values: Vec<f64>,
}

impl Reader {
    fn new(dims: Dimensions) -> Self {
        let m_index = match dims {
            Dimensions::Xym => Some(2),
            Dimensions::Xyzm => Some(3),
            Dimensions::Xy | Dimensions::Xyz | Dimensions::Unknown(_) => None,
        };
        Self {
            m_index,
            m_values: Vec::new(),
        }
    }

    fn finish(self) -> Option<Vec<f64>> {
        self.m_index.is_some().then_some(self.m_values)
    }

    fn coord(&mut self, coord: &impl CoordTrait<T = f64>) -> Result<Coord<i32>, TileWkbError> {
        let x = ordinate(coord.x())?;
        let y = ordinate(coord.y())?;
        if let Some(index) = self.m_index {
            self.m_values
                .push(coord.nth(index).ok_or(TileWkbError::MissingMeasure)?);
        }
        Ok(Coord { x, y })
    }

    fn point(&mut self, point: &impl PointTrait<T = f64>) -> Result<Point<i32>, TileWkbError> {
        let coord = point.coord().ok_or(TileWkbError::EmptyPoint)?;
        Ok(Point(self.coord(&coord)?))
    }

    fn line(
        &mut self,
        line: &impl LineStringTrait<T = f64>,
    ) -> Result<LineString<i32>, TileWkbError> {
        let mut coords = Vec::with_capacity(line.num_coords());
        for coord in line.coords() {
            coords.push(self.coord(&coord)?);
        }
        Ok(LineString(coords))
    }

    fn polygon(
        &mut self,
        polygon: &impl PolygonTrait<T = f64>,
    ) -> Result<Polygon<i32>, TileWkbError> {
        let Some(exterior) = polygon.exterior() else {
            return Ok(Polygon::new(LineString(Vec::new()), Vec::new()));
        };
        let exterior = self.line(&exterior)?;
        let mut interiors = Vec::with_capacity(polygon.num_interiors());
        for ring in polygon.interiors() {
            interiors.push(self.line(&ring)?);
        }
        Ok(Polygon::new(exterior, interiors))
    }

    fn geometry(&mut self, wkb: &Wkb<'_>) -> Result<Geometry<i32>, TileWkbError> {
        let unsupported = |name| Err(TileWkbError::UnsupportedGeometry(name));
        match wkb.as_type() {
            GeometryType::Point(g) => Ok(Geometry::Point(self.point(g)?)),
            GeometryType::LineString(g) => Ok(Geometry::LineString(self.line(g)?)),
            GeometryType::Polygon(g) => Ok(Geometry::Polygon(self.polygon(g)?)),
            GeometryType::MultiPoint(g) => {
                let mut points = Vec::with_capacity(g.num_points());
                for point in g.points() {
                    points.push(self.point(&point)?);
                }
                Ok(match <[_; 1]>::try_from(points) {
                    Ok([point]) => Geometry::Point(point),
                    Err(points) => Geometry::MultiPoint(MultiPoint(points)),
                })
            }
            GeometryType::MultiLineString(g) => {
                let mut lines = Vec::with_capacity(g.num_line_strings());
                for line in g.line_strings() {
                    lines.push(self.line(&line)?);
                }
                Ok(match <[_; 1]>::try_from(lines) {
                    Ok([line]) => Geometry::LineString(line),
                    Err(lines) => Geometry::MultiLineString(MultiLineString(lines)),
                })
            }
            GeometryType::MultiPolygon(g) => {
                let mut polygons = Vec::with_capacity(g.num_polygons());
                for polygon in g.polygons() {
                    polygons.push(self.polygon(&polygon)?);
                }
                Ok(match <[_; 1]>::try_from(polygons) {
                    Ok([polygon]) => Geometry::Polygon(polygon),
                    Err(polygons) => Geometry::MultiPolygon(MultiPolygon(polygons)),
                })
            }
            GeometryType::GeometryCollection(_) => unsupported("GeometryCollection"),
            GeometryType::Rect(_) => unsupported("Rect"),
            GeometryType::Triangle(_) => unsupported("Triangle"),
            GeometryType::Line(_) => unsupported("Line"),
        }
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "the rounded value is bounds-checked against i32 before the cast"
)]
fn ordinate(value: f64) -> Result<i32, TileWkbError> {
    let rounded = value.round();
    if !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&rounded) {
        return Err(TileWkbError::CoordinateOutOfRange(value));
    }
    Ok(rounded as i32)
}

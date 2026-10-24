//! What a data source hands the generator: partitions of features streamed in batches.

use bytes::Bytes;
use geo_traits::to_geo::{ToGeoCoord as _, ToGeoGeometry as _, ToGeoLineString, ToGeoPolygon};
use geo_traits::{
    GeometryTrait as _, GeometryType, MultiLineStringTrait as _, MultiPointTrait as _, MultiPolygonTrait as _,
    PointTrait as _,
};
use geo_types::{Coord, Geometry as GeoGeometry, GeometryCollection, LineString, Polygon};
use wkb::reader::Wkb;

use crate::FeatureGeom;
use crate::props::{KeyId, KeyInterner, Prop, SourceStr};

/// Result of reading a [`FeatureSource`].
pub type SourceResult<T> = Result<T, SourceError>;

/// Why reading a [`FeatureSource`] failed.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum SourceError {
    /// The source has no such partition.
    #[error("partition {0} does not exist")]
    UnknownPartition(u32),

    /// The consumer of the batches stopped early.
    #[error("the tile writer stopped early")]
    Stopped,

    /// The source itself failed.
    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

/// Coordinate system of a batch's geometry; projection to Web Mercator happens on render workers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(missing_docs)]
pub enum Crs {
    Wgs84,
    WebMercator,
}

/// The geometry of one feature.
#[derive(Clone, Debug, PartialEq)]
#[expect(missing_docs)]
pub enum Geometry {
    Point(Coord<f64>),
    Line(LineString<f64>),
    Polygon(Polygon<f64>),
    Points(Vec<Coord<f64>>),
    Lines(Vec<LineString<f64>>),
    Polygons(Vec<Polygon<f64>>),
    /// Well-known binary, decoded by [`Geometry::decode_wkb`] on the render workers.
    Wkb(Bytes),
}

impl Geometry {
    /// Decodes [`Geometry::Wkb`] in place, dropping the empty points of a multipoint; `Ok(false)` if it is
    /// an empty point or an empty geometry collection, which have nothing to draw.
    ///
    /// # Errors
    ///
    /// If the WKB is malformed or holds a geometry a tile cannot, such as a geometry collection or a triangle.
    pub fn decode_wkb(&mut self) -> Result<bool, WkbError> {
        let Self::Wkb(bytes) = self else {
            return Ok(true);
        };
        match decode_wkb(bytes)? {
            Some(geometry) => {
                *self = geometry;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// The renderable view of the geometry; `None` while it is still [`Geometry::Wkb`].
    #[must_use]
    pub fn feature_geom(&self) -> Option<FeatureGeom<'_>> {
        Some(match self {
            Self::Point(point) => FeatureGeom::Points(std::slice::from_ref(point)),
            Self::Line(line) => FeatureGeom::Lines(std::slice::from_ref(line)),
            Self::Polygon(polygon) => FeatureGeom::Polygons(std::slice::from_ref(polygon)),
            Self::Points(points) => FeatureGeom::Points(points),
            Self::Lines(lines) => FeatureGeom::Lines(lines),
            Self::Polygons(polygons) => FeatureGeom::Polygons(polygons),
            Self::Wkb(_) => return None,
        })
    }
}

/// Why a WKB geometry cannot be the geometry of a feature.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WkbError {
    /// A well-formed geometry of a type tiles cannot hold.
    #[error("a {kind} cannot go into a tile, which holds only points, lines and polygons (single or multi)")]
    Unsupported { kind: UnsupportedGeometry },

    /// The bytes are not one WKB geometry.
    #[error("malformed WKB ({len} bytes): {reason}")]
    Malformed { len: usize, reason: String },
}

/// A WKB geometry type that is not a point, line or polygon, single or multi, named as WKT names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
#[expect(missing_docs)]
pub enum UnsupportedGeometry {
    GeometryCollection,
    CircularString,
    CompoundCurve,
    CurvePolygon,
    MultiCurve,
    MultiSurface,
    Curve,
    Surface,
    PolyhedralSurface,
    Tin,
    Triangle,
}

impl UnsupportedGeometry {
    /// The type of an ISO WKB type code without its dimension, `None` for the six supported ones and unknown codes.
    fn of_code(code: u32) -> Option<Self> {
        Some(match code {
            7 => Self::GeometryCollection,
            8 => Self::CircularString,
            9 => Self::CompoundCurve,
            10 => Self::CurvePolygon,
            11 => Self::MultiCurve,
            12 => Self::MultiSurface,
            13 => Self::Curve,
            14 => Self::Surface,
            15 => Self::PolyhedralSurface,
            16 => Self::Tin,
            17 => Self::Triangle,
            _ => return None,
        })
    }
}

impl std::fmt::Display for UnsupportedGeometry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::GeometryCollection => "GEOMETRYCOLLECTION",
            Self::CircularString => "CIRCULARSTRING",
            Self::CompoundCurve => "COMPOUNDCURVE",
            Self::CurvePolygon => "CURVEPOLYGON",
            Self::MultiCurve => "MULTICURVE",
            Self::MultiSurface => "MULTISURFACE",
            Self::Curve => "CURVE",
            Self::Surface => "SURFACE",
            Self::PolyhedralSurface => "POLYHEDRALSURFACE",
            Self::Tin => "TIN",
            Self::Triangle => "TRIANGLE",
        })
    }
}

/// Why WKB shorter than its byte order and type code is malformed.
const HEADER: &str = "a geometry starts with a 5-byte header of byte order and type code";

/// The EWKB flags for Z, M and an SRID, set on the type code instead of the ISO dimension offsets.
const EWKB_FLAGS: u32 = 0xE000_0000;

/// The EWKB flag of a type code followed by an SRID.
const EWKB_SRID: u32 = 0x2000_0000;

/// Checks the type code itself, since the `wkb` crate reads it as `code & 0x7` and so takes a TRIANGLE (17)
/// for a point; georust/wkb#104. Converts each part of a multi-geometry on its own, so one empty point does
/// not fail the whole multipoint.
fn decode_wkb(bytes: &[u8]) -> Result<Option<Geometry>, WkbError> {
    let malformed = |reason: String| WkbError::Malformed {
        len: bytes.len(),
        reason,
    };
    match bytes.first() {
        None => return Err(malformed(HEADER.to_owned())),
        Some(0 | 1) => {}
        Some(order) => {
            return Err(malformed(format!(
                "byte order {order} is neither 0 (big endian) nor 1 (little endian)"
            )));
        }
    }
    let code = read_u32(bytes, 1).ok_or_else(|| malformed(HEADER.to_owned()))?;
    let kind = (code & !EWKB_FLAGS) % 1000;
    if let Some(kind) = UnsupportedGeometry::of_code(kind) {
        let parts = if code & EWKB_SRID == 0 { 5 } else { 9 };
        return match (kind, read_u32(bytes, parts)) {
            (UnsupportedGeometry::GeometryCollection, Some(0)) => Ok(None),
            _ => Err(WkbError::Unsupported { kind }),
        };
    }
    if !(1..=6).contains(&kind) {
        return Err(malformed(format!("type code {code} names no WKB geometry type")));
    }
    let wkb = Wkb::try_new(bytes).map_err(|err| {
        if let wkb::error::WkbError::General(reason) = err {
            malformed(reason)
        } else {
            malformed(err.to_string())
        }
    })?;
    let end = wkb.buf().len();
    if end < bytes.len() {
        return Err(malformed(format!("the geometry ends after byte {end}")));
    }
    Ok(match wkb.as_type() {
        GeometryType::MultiPoint(points) => Some(Geometry::Points(
            points
                .points()
                .filter_map(|p| p.coord())
                .map(|c| c.to_coord())
                .collect(),
        )),
        GeometryType::MultiLineString(lines) => Some(Geometry::Lines(
            lines.line_strings().map(ToGeoLineString::to_line_string).collect(),
        )),
        GeometryType::MultiPolygon(polygons) => Some(Geometry::Polygons(
            polygons.polygons().map(ToGeoPolygon::to_polygon).collect(),
        )),
        GeometryType::Point(_)
        | GeometryType::LineString(_)
        | GeometryType::Polygon(_)
        | GeometryType::GeometryCollection(_)
        | GeometryType::Rect(_)
        | GeometryType::Triangle(_)
        | GeometryType::Line(_) => wkb.try_to_geometry().and_then(|geometry| geometry.try_into().ok()),
    })
}

/// The `u32` at `offset` in the byte order of the WKB's first byte, `None` past the end of `bytes`.
fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let word: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
    Some(if bytes[0] == 0 {
        u32::from_be_bytes(word)
    } else {
        u32::from_le_bytes(word)
    })
}

impl TryFrom<GeoGeometry<f64>> for Geometry {
    /// A geometry collection, which has no single feature geometry.
    type Error = GeometryCollection<f64>;

    fn try_from(geometry: GeoGeometry<f64>) -> Result<Self, Self::Error> {
        Ok(match geometry {
            GeoGeometry::Point(p) => Self::Point(p.0),
            GeoGeometry::MultiPoint(mp) => Self::Points(mp.0.into_iter().map(|p| p.0).collect()),
            GeoGeometry::LineString(ls) => Self::Line(ls),
            GeoGeometry::MultiLineString(mls) => Self::Lines(mls.0),
            GeoGeometry::Polygon(polygon) => Self::Polygon(polygon),
            GeoGeometry::MultiPolygon(mp) => Self::Polygons(mp.0),
            GeoGeometry::Line(line) => Self::Line(line.into()),
            GeoGeometry::Rect(rect) => Self::Polygon(rect.to_polygon()),
            GeoGeometry::Triangle(triangle) => Self::Polygon(triangle.to_polygon()),
            GeoGeometry::GeometryCollection(collection) => return Err(collection),
        })
    }
}

/// One feature as a source reads it.
#[derive(Clone, Debug, PartialEq)]
#[expect(missing_docs)]
pub struct SourceFeature {
    pub id: Option<u64>,
    pub geometry: Geometry,
    pub props: Vec<(KeyId, Prop<SourceStr>)>,
}

/// Consecutive features of one partition and table: feature `i` is row `first_row + i`.
#[derive(Clone, Debug)]
#[expect(missing_docs)]
pub struct FeatureBatch {
    /// Position of the table in the generator's plan.
    pub table: u16,
    pub partition: u32,
    pub first_row: u64,
    pub crs: Crs,
    pub features: Vec<SourceFeature>,
}

/// A source split into partitions that can be read in parallel. Reading partitions in index order,
/// each in its own row order, must reproduce the source's order: that is the draw order the output
/// keeps, deterministically, whatever the thread timing.
pub trait FeatureSource: Sync {
    /// How many partitions the source is split into.
    fn partitions(&self) -> u32;

    /// Streams one partition. `keys` is parallel to the plan's tables; keys not declared up front are
    /// interned through it. Async sources block on their own runtime here.
    ///
    /// # Errors
    /// If reading fails or `emit` does.
    fn read(
        &self,
        partition: u32,
        keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
    ) -> SourceResult<()>;

    /// Up to `count` readers, each on its own thread. Sources that must read every partition from one
    /// consistent state open their per-reader state here, all at once, so that state can be pinned before
    /// any partition is read.
    ///
    /// # Errors
    /// If opening the readers fails.
    fn open_readers(&self, count: usize) -> SourceResult<Vec<Box<dyn FeatureReader + '_>>>
    where
        Self: Sized,
    {
        Ok((0..count)
            .map(|_| Box::new(Stateless(self)) as Box<dyn FeatureReader + '_>)
            .collect())
    }
}

/// What one reader thread reads partitions through; see [`FeatureSource::open_readers`].
pub trait FeatureReader: Send {
    /// Same contract as [`FeatureSource::read`].
    ///
    /// # Errors
    /// If reading fails or `emit` does.
    fn read(
        &mut self,
        partition: u32,
        keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
    ) -> SourceResult<()>;
}

struct Stateless<'a, S>(&'a S);

impl<S: FeatureSource> FeatureReader for Stateless<'_, S> {
    fn read(
        &mut self,
        partition: u32,
        keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
    ) -> SourceResult<()> {
        self.0.read(partition, keys, emit)
    }
}

/// A source over prepared batches, for tests and small datasets; each batch is its own partition.
#[derive(Debug)]
pub struct MemorySource {
    /// One batch per partition.
    pub batches: Vec<FeatureBatch>,
}

impl FeatureSource for MemorySource {
    fn partitions(&self) -> u32 {
        u32::try_from(self.batches.len()).expect("fewer than 2^32 batches")
    }

    fn read(
        &self,
        partition: u32,
        _keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
    ) -> SourceResult<()> {
        let batch = self
            .batches
            .get(partition as usize)
            .ok_or(SourceError::UnknownPartition(partition))?;
        emit(FeatureBatch {
            partition,
            ..batch.clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use geo_traits::GeometryTrait;
    use geo_types::wkt;
    use wkt::types::{Dimension, MultiPoint, Point};

    use super::*;

    fn wkb(geometry: &impl GeometryTrait<T = f64>) -> Geometry {
        let mut bytes = Vec::new();
        wkb::writer::write_geometry(&mut bytes, geometry, &wkb::writer::WriteOptions::default()).unwrap();
        Geometry::Wkb(Bytes::from(bytes))
    }

    fn hex(text: &str) -> Geometry {
        let bytes = (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(text.get(i..i + 2).unwrap(), 16).unwrap())
            .collect::<Vec<u8>>();
        Geometry::Wkb(Bytes::from(bytes))
    }

    fn point(x: f64, y: f64) -> Point<f64> {
        Point::from_coord(wkt::types::Coord { x, y, z: None, m: None })
    }

    fn decode_error(mut geometry: Geometry) -> String {
        geometry.decode_wkb().unwrap_err().to_string()
    }

    #[test]
    fn wkb_point_decodes() {
        let mut geometry = hex("0101000000000000000000f03f0000000000000040");
        assert_eq!(geometry.decode_wkb(), Ok(true));
        assert_eq!(geometry, Geometry::Point(Coord { x: 1.0, y: 2.0 }));
    }

    #[test]
    fn big_endian_wkb_line_decodes() {
        let mut geometry = hex("000000000200000002000000000000000000000000000000003ff00000000000003ff0000000000000");
        assert_eq!(geometry.decode_wkb(), Ok(true));
        assert_eq!(geometry, Geometry::Line(LineString::from(vec![(0.0, 0.0), (1.0, 1.0)])));
    }

    #[test]
    fn wkb_multipoint_keeps_its_real_points() {
        let points = vec![point(1., 2.), Point::empty(Dimension::XY), point(3., 4.)];
        let mut geometry = wkb(&MultiPoint::new(points, Dimension::XY));
        assert_eq!(geometry.decode_wkb(), Ok(true));
        let expected = Geometry::try_from(GeoGeometry::from(wkt! { MULTIPOINT(1. 2., 3. 4.) })).unwrap();
        assert_eq!(geometry, expected);
    }

    #[test]
    fn wkb_empty_point_is_skipped() {
        assert_eq!(wkb(&Point::<f64>::empty(Dimension::XY)).decode_wkb(), Ok(false));
    }

    #[test]
    fn wkb_empty_geometry_collection_is_skipped() {
        assert_eq!(hex("010700000000000000").decode_wkb(), Ok(false));
    }

    #[test]
    fn ewkb_empty_geometry_collection_with_srid_is_skipped() {
        assert_eq!(hex("0107000020110f000000000000").decode_wkb(), Ok(false));
    }

    #[test]
    fn wkb_geometry_collection_is_rejected() {
        insta::assert_snapshot!(
            decode_error(hex("0107000000010000000101000000000000000000f03f000000000000f03f")),
            @"a GEOMETRYCOLLECTION cannot go into a tile, which holds only points, lines and polygons (single or multi)"
        );
    }

    #[test]
    fn wkb_triangle_is_rejected() {
        insta::assert_snapshot!(
            decode_error(hex("0111000000010000000400000000000000000000000000000000000000000000000000f03f00000000000000000000000000000000000000000000f03f00000000000000000000000000000000")),
            @"a TRIANGLE cannot go into a tile, which holds only points, lines and polygons (single or multi)"
        );
    }

    #[test]
    fn iso_wkb_triangle_z_is_rejected() {
        insta::assert_snapshot!(
            decode_error(hex("01f9030000010000000400000000000000000000000000000000000000000000000000f03f000000000000f03f0000000000000000000000000000f03f0000000000000000000000000000f03f000000000000f03f00000000000000000000000000000000000000000000f03f")),
            @"a TRIANGLE cannot go into a tile, which holds only points, lines and polygons (single or multi)"
        );
    }

    #[test]
    fn ewkb_triangle_with_srid_is_rejected() {
        insta::assert_snapshot!(
            decode_error(hex("0111000020e6100000010000000400000000000000000000000000000000000000000000000000f03f00000000000000000000000000000000000000000000f03f00000000000000000000000000000000")),
            @"a TRIANGLE cannot go into a tile, which holds only points, lines and polygons (single or multi)"
        );
    }

    #[test]
    fn wkb_tin_is_rejected() {
        insta::assert_snapshot!(
            decode_error(hex("0110000000010000000111000000010000000400000000000000000000000000000000000000000000000000f03f00000000000000000000000000000000000000000000f03f00000000000000000000000000000000")),
            @"a TIN cannot go into a tile, which holds only points, lines and polygons (single or multi)"
        );
    }

    #[test]
    fn wkb_polyhedral_surface_is_rejected() {
        insta::assert_snapshot!(
            decode_error(hex("010f000000010000000103000000010000000400000000000000000000000000000000000000000000000000f03f00000000000000000000000000000000000000000000f03f00000000000000000000000000000000")),
            @"a POLYHEDRALSURFACE cannot go into a tile, which holds only points, lines and polygons (single or multi)"
        );
    }

    #[test]
    fn wkb_circular_string_is_rejected() {
        insta::assert_snapshot!(
            decode_error(hex("01080000000300000000000000000000000000000000000000000000000000f03f000000000000f03f00000000000000400000000000000000")),
            @"a CIRCULARSTRING cannot go into a tile, which holds only points, lines and polygons (single or multi)"
        );
    }

    #[test]
    fn no_bytes_are_malformed_wkb() {
        insta::assert_snapshot!(decode_error(hex("")), @"malformed WKB (0 bytes): a geometry starts with a 5-byte header of byte order and type code");
    }

    #[test]
    fn unknown_byte_order_is_malformed_wkb() {
        insta::assert_snapshot!(decode_error(hex("0201000000000000000000f03f0000000000000040")), @"malformed WKB (21 bytes): byte order 2 is neither 0 (big endian) nor 1 (little endian)");
    }

    #[test]
    fn cut_header_is_malformed_wkb() {
        insta::assert_snapshot!(decode_error(hex("010100")), @"malformed WKB (3 bytes): a geometry starts with a 5-byte header of byte order and type code");
    }

    #[test]
    fn unknown_type_code_is_malformed_wkb() {
        insta::assert_snapshot!(decode_error(hex("0163000000")), @"malformed WKB (5 bytes): type code 99 names no WKB geometry type");
    }

    #[test]
    fn cut_point_is_malformed_wkb() {
        insta::assert_snapshot!(decode_error(hex("0101000000000000000000f03f")), @"malformed WKB (13 bytes): Invalid buffer length for Point: geometry would end at byte 21, but buffer length is 13.");
    }

    #[test]
    fn bytes_after_the_geometry_are_malformed_wkb() {
        insta::assert_snapshot!(decode_error(hex("0101000000000000000000f03f0000000000000040ff")), @"malformed WKB (22 bytes): the geometry ends after byte 21");
    }
}

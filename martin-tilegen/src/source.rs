//! What a data source hands the generator: partitions of features streamed in batches.

use bytes::Bytes;
use geo_traits::to_geo::{ToGeoCoord as _, ToGeoGeometry as _, ToGeoLineString, ToGeoPolygon};
use geo_traits::{
    GeometryCollectionTrait as _, GeometryTrait, GeometryType, LineStringTrait as _, MultiLineStringTrait as _,
    MultiPointTrait as _, MultiPolygonTrait as _, PointTrait as _, PolygonTrait as _,
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
    /// If the `wkb` crate cannot read the WKB, or it holds a geometry collection that is not empty.
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
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum WkbError {
    /// A geometry collection that is not empty, which tiles cannot hold.
    #[error(
        "a GEOMETRYCOLLECTION cannot go into a tile, which holds only points, lines and polygons (single or multi)"
    )]
    GeometryCollection,

    /// The `wkb` crate cannot read the bytes.
    #[error("cannot read the WKB: {error}")]
    Read {
        #[source]
        error: wkb::error::WkbError,
    },
}

/// Converts each part of a multi-geometry on its own, so one empty point does not fail the whole multipoint.
fn decode_wkb(bytes: &[u8]) -> Result<Option<Geometry>, WkbError> {
    let wkb = Wkb::try_new(bytes).map_err(|error| WkbError::Read { error })?;
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
        GeometryType::GeometryCollection(_) if is_empty(&wkb) => None,
        GeometryType::GeometryCollection(_) => return Err(WkbError::GeometryCollection),
        GeometryType::Point(_)
        | GeometryType::LineString(_)
        | GeometryType::Polygon(_)
        | GeometryType::Rect(_)
        | GeometryType::Triangle(_)
        | GeometryType::Line(_) => wkb.try_to_geometry().and_then(|geometry| geometry.try_into().ok()),
    })
}

/// Whether `geometry` has no coordinates, as `ST_IsEmpty` decides: a collection is empty when all its parts are.
fn is_empty(geometry: &impl GeometryTrait<T = f64>) -> bool {
    match geometry.as_type() {
        GeometryType::Point(point) => point.coord().is_none(),
        GeometryType::LineString(line) => line.num_coords() == 0,
        GeometryType::Polygon(polygon) => polygon.exterior().is_none_or(|ring| ring.num_coords() == 0),
        GeometryType::MultiPoint(points) => points.points().all(|point| point.coord().is_none()),
        GeometryType::MultiLineString(lines) => lines.line_strings().all(|line| line.num_coords() == 0),
        GeometryType::MultiPolygon(polygons) => polygons
            .polygons()
            .all(|polygon| polygon.exterior().is_none_or(|ring| ring.num_coords() == 0)),
        GeometryType::GeometryCollection(collection) => collection.geometries().all(|part| is_empty(&part)),
        GeometryType::Rect(_) | GeometryType::Triangle(_) | GeometryType::Line(_) => false,
    }
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
        assert!(geometry.decode_wkb().unwrap());
        assert_eq!(geometry, Geometry::Point(Coord { x: 1.0, y: 2.0 }));
    }

    #[test]
    fn big_endian_wkb_line_decodes() {
        let mut geometry = hex("000000000200000002000000000000000000000000000000003ff00000000000003ff0000000000000");
        assert!(geometry.decode_wkb().unwrap());
        assert_eq!(geometry, Geometry::Line(LineString::from(vec![(0.0, 0.0), (1.0, 1.0)])));
    }

    #[test]
    fn wkb_multipoint_keeps_its_real_points() {
        let points = vec![point(1., 2.), Point::empty(Dimension::XY), point(3., 4.)];
        let mut geometry = wkb(&MultiPoint::new(points, Dimension::XY));
        assert!(geometry.decode_wkb().unwrap());
        let expected = Geometry::try_from(GeoGeometry::from(wkt! { MULTIPOINT(1. 2., 3. 4.) })).unwrap();
        assert_eq!(geometry, expected);
    }

    #[test]
    fn wkb_empty_point_is_skipped() {
        assert!(!wkb(&Point::<f64>::empty(Dimension::XY)).decode_wkb().unwrap());
    }

    #[test]
    fn wkb_empty_geometry_collection_is_skipped() {
        assert!(!hex("010700000000000000").decode_wkb().unwrap());
    }

    #[test]
    fn ewkb_empty_geometry_collection_with_srid_is_skipped() {
        assert!(!hex("0107000020110f000000000000").decode_wkb().unwrap());
    }

    #[test]
    fn wkb_geometry_collection_of_empty_parts_is_skipped() {
        let parts = vec![
            wkt::Wkt::Point(Point::empty(Dimension::XY)),
            wkt::Wkt::GeometryCollection(wkt::types::GeometryCollection::empty(Dimension::XY)),
        ];
        assert!(
            !wkb(&wkt::types::GeometryCollection::new(parts, Dimension::XY))
                .decode_wkb()
                .unwrap()
        );
    }

    #[test]
    fn wkb_geometry_collection_is_rejected() {
        insta::assert_snapshot!(
            decode_error(hex("0107000000010000000101000000000000000000f03f000000000000f03f")),
            @"a GEOMETRYCOLLECTION cannot go into a tile, which holds only points, lines and polygons (single or multi)"
        );
    }

    #[test]
    fn wkb_triangle_is_read_as_a_point() {
        // FIXME: georust/wkb#104 rejects type code 17; once a wkb bump brings it, this fails: expect "WKB type code out of range. Got: 17" again
        let mut geometry = hex(
            "0111000000010000000400000000000000000000000000000000000000000000000000f03f00000000000000000000000000000000000000000000f03f00000000000000000000000000000000",
        );
        assert!(geometry.decode_wkb().unwrap());
        insta::assert_debug_snapshot!(geometry, @"
        Point(
            COORD(8.4879831644e-314 0.0),
        )
        ");
    }

    #[test]
    fn cut_point_fails_with_the_wkb_crate_error() {
        insta::assert_snapshot!(decode_error(hex("0101000000000000000000f03f")), @"cannot read the WKB: General error: Invalid buffer length for Point: geometry would end at byte 21, but buffer length is 13.");
    }
}

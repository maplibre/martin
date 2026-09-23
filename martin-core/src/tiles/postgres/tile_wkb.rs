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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn wkb(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .chunks(2)
            .map(|pair| {
                u8::from_str_radix(
                    std::str::from_utf8(pair).expect("fixture is valid utf-8"),
                    16,
                )
                .expect("fixture is valid hex")
            })
            .collect()
    }

    fn parse(hex: &str) -> Geometry<i32> {
        parse_tile_wkb(&wkb(hex)).expect("fixture should parse").0
    }

    fn measures(hex: &str) -> Option<Vec<f64>> {
        parse_tile_wkb(&wkb(hex)).expect("fixture should parse").1
    }

    fn err(hex: &str) -> TileWkbError {
        parse_tile_wkb(&wkb(hex)).expect_err("fixture should not parse")
    }

    fn c(x: i32, y: i32) -> Coord<i32> {
        Coord { x, y }
    }

    fn line(coords: &[(i32, i32)]) -> LineString<i32> {
        LineString(coords.iter().map(|&(x, y)| c(x, y)).collect())
    }

    #[test]
    fn point() {
        assert_eq!(
            parse("010100000000000000000024400000000000003440"),
            Geometry::Point(Point(c(10, 20)))
        );
    }

    #[test]
    fn point_with_negative_coordinates() {
        assert_eq!(
            parse("010100000000000000000050c000000000000050c0"),
            Geometry::Point(Point(c(-64, -64)))
        );
    }

    #[test]
    fn linestring() {
        assert_eq!(
            parse(
                "010200000003000000000000000000000000000000000000000000000000002440000000000000244000000000000034400000000000001440"
            ),
            Geometry::LineString(line(&[(0, 0), (10, 10), (20, 5)]))
        );
    }

    #[test]
    fn polygon_with_interior_ring() {
        assert_eq!(
            parse(
                "010300000002000000050000000000000000000000000000000000000000000000000059400000000000000000000000000000594000000000000059400000000000000000000000000000594000000000000000000000000000000000050000000000000000002440000000000000244000000000000034400000000000002440000000000000344000000000000034400000000000002440000000000000344000000000000024400000000000002440"
            ),
            Geometry::Polygon(Polygon::new(
                line(&[(0, 0), (100, 0), (100, 100), (0, 100), (0, 0)]),
                vec![line(&[(10, 10), (20, 10), (20, 20), (10, 20), (10, 10)])],
            ))
        );
    }

    #[test]
    fn multipoint() {
        assert_eq!(
            parse(
                "0104000000020000000101000000000000000000f03f0000000000000040010100000000000000000008400000000000001040"
            ),
            Geometry::MultiPoint(MultiPoint(vec![Point(c(1, 2)), Point(c(3, 4))]))
        );
    }

    #[test]
    fn multilinestring() {
        assert_eq!(
            parse(
                "01050000000200000001020000000200000000000000000000000000000000000000000000000000f03f000000000000f03f010200000003000000000000000000004000000000000000400000000000000840000000000000084000000000000010400000000000001040"
            ),
            Geometry::MultiLineString(MultiLineString(vec![
                line(&[(0, 0), (1, 1)]),
                line(&[(2, 2), (3, 3), (4, 4)]),
            ]))
        );
    }

    #[test]
    fn multipolygon() {
        assert_eq!(
            parse(
                "010600000002000000010300000001000000040000000000000000000000000000000000000000000000000024400000000000000000000000000000244000000000000024400000000000000000000000000000000001030000000100000004000000000000000000344000000000000034400000000000003e4000000000000034400000000000003e400000000000003e4000000000000034400000000000003440"
            ),
            Geometry::MultiPolygon(MultiPolygon(vec![
                Polygon::new(line(&[(0, 0), (10, 0), (10, 10), (0, 0)]), Vec::new()),
                Polygon::new(line(&[(20, 20), (30, 20), (30, 30), (20, 20)]), Vec::new()),
            ]))
        );
    }

    #[test]
    fn xym_point_keeps_measure() {
        let hex = "01d1070000000000000000084000000000000010400000000000001c40";
        assert_eq!(parse(hex), Geometry::Point(Point(c(3, 4))));
        assert_eq!(measures(hex), Some(vec![7.0]));
    }

    #[test]
    fn xym_linestring_keeps_measure() {
        let hex = "01d207000002000000000000000000000000000000000000000000000000002440000000000000f03f000000000000f03f0000000000003440";
        assert_eq!(parse(hex), Geometry::LineString(line(&[(0, 0), (1, 1)])));
        assert_eq!(measures(hex), Some(vec![10.0, 20.0]));
    }

    #[test]
    fn xyzm_linestring_drops_z_keeps_measure() {
        let hex = "01ba0b0000020000000000000000000000000000000000000000000000000014400000000000002440000000000000f03f000000000000f03f00000000000018400000000000003440";
        assert_eq!(parse(hex), Geometry::LineString(line(&[(0, 0), (1, 1)])));
        assert_eq!(measures(hex), Some(vec![10.0, 20.0]));
    }

    #[test]
    fn xyz_linestring_drops_z_without_measure() {
        let hex = "01ea03000002000000000000000000000000000000000000000000000000001440000000000000f03f000000000000f03f0000000000001840";
        assert_eq!(parse(hex), Geometry::LineString(line(&[(0, 0), (1, 1)])));
        assert_eq!(measures(hex), None);
    }

    #[test]
    fn xym_multipoint_keeps_measure() {
        let hex = "01d40700000200000001d1070000000000000000f03f0000000000000040000000000000084001d1070000000000000000104000000000000014400000000000001840";
        assert_eq!(
            parse(hex),
            Geometry::MultiPoint(MultiPoint(vec![Point(c(1, 2)), Point(c(4, 5))]))
        );
        assert_eq!(measures(hex), Some(vec![3.0, 6.0]));
    }

    #[test]
    fn xym_multipolygon_keeps_measure() {
        let hex = "01d60700000100000001d3070000020000000400000000000000000000000000000000000000000000000000f03f00000000000010400000000000000000000000000000004000000000000010400000000000001040000000000000084000000000000000000000000000000000000000000000104004000000000000000000f03f000000000000f03f00000000000014400000000000000040000000000000f03f0000000000001840000000000000004000000000000000400000000000001c40000000000000f03f000000000000f03f0000000000002040";
        assert_eq!(
            parse(hex),
            Geometry::Polygon(Polygon::new(
                line(&[(0, 0), (4, 0), (4, 4), (0, 0)]),
                vec![line(&[(1, 1), (2, 1), (2, 2), (1, 1)])],
            ))
        );
        assert_eq!(
            measures(hex),
            Some(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0])
        );
    }

    #[test]
    fn big_endian_point() {
        assert_eq!(
            parse("000000000140240000000000004034000000000000"),
            Geometry::Point(Point(c(10, 20)))
        );
    }

    #[test]
    fn big_endian_polygon() {
        assert_eq!(
            parse(
                "0000000003000000010000000400000000000000000000000000000000401000000000000000000000000000004010000000000000401000000000000000000000000000000000000000000000"
            ),
            Geometry::Polygon(Polygon::new(
                line(&[(0, 0), (4, 0), (4, 4), (0, 0)]),
                Vec::new()
            ))
        );
    }

    #[test]
    fn big_endian_xym_linestring() {
        let hex = "00000007d2000000020000000000000000000000000000000040240000000000003ff00000000000003ff00000000000004034000000000000";
        assert_eq!(parse(hex), Geometry::LineString(line(&[(0, 0), (1, 1)])));
        assert_eq!(measures(hex), Some(vec![10.0, 20.0]));
    }

    #[test]
    fn ewkb_srid_is_skipped() {
        assert_eq!(
            parse("0101000020110f000000000000000024400000000000003440"),
            Geometry::Point(Point(c(10, 20)))
        );
    }

    #[test]
    fn ewkb_srid_and_measure_flags() {
        let hex = "0102000060110f000002000000000000000000000000000000000000000000000000002440000000000000f03f000000000000f03f0000000000003440";
        assert_eq!(parse(hex), Geometry::LineString(line(&[(0, 0), (1, 1)])));
        assert_eq!(measures(hex), Some(vec![10.0, 20.0]));
    }

    #[test]
    fn ewkb_srid_with_zm_flags_drops_z() {
        let hex = "01030000e0e6100000010000000400000000000000000000000000000000000000000000000000f03f0000000000000040000000000000f03f000000000000000000000000000008400000000000001040000000000000f03f000000000000f03f00000000000014400000000000001840000000000000000000000000000000000000000000001c400000000000002040";
        assert_eq!(
            parse(hex),
            Geometry::Polygon(Polygon::new(
                line(&[(0, 0), (1, 0), (1, 1), (0, 0)]),
                Vec::new()
            ))
        );
        assert_eq!(measures(hex), Some(vec![2.0, 4.0, 6.0, 8.0]));
    }

    #[rstest]
    #[case::linestring("010200000000000000", Geometry::LineString(LineString(Vec::new())))]
    #[case::polygon(
        "010300000000000000",
        Geometry::Polygon(Polygon::new(LineString(Vec::new()), Vec::new()))
    )]
    #[case::multipoint("010400000000000000", Geometry::MultiPoint(MultiPoint(Vec::new())))]
    fn empty_geometries_parse(#[case] hex: &str, #[case] expected: Geometry<i32>) {
        assert_eq!(parse(hex), expected);
    }

    #[test]
    fn multilinestring_with_an_empty_part() {
        assert_eq!(
            parse(
                "010500000002000000010200000000000000010200000002000000000000000000f03f000000000000f03f00000000000000400000000000000040"
            ),
            Geometry::MultiLineString(MultiLineString(vec![
                LineString(Vec::new()),
                line(&[(1, 1), (2, 2)]),
            ]))
        );
    }

    #[test]
    fn st_asmvtgeom_linestring_keeps_measure() {
        let hex = "01d207000002000000000000000000a040000000000000a0400000000000002440000000000016a0400000000000d49f400000000000003440";
        assert_eq!(
            parse(hex),
            Geometry::LineString(line(&[(2048, 2048), (2059, 2037)]))
        );
        assert_eq!(measures(hex), Some(vec![10.0, 20.0]));
    }

    #[test]
    fn st_asmvtgeom_polygon_with_interior_ring() {
        assert_eq!(
            parse(
                "01030000000200000005000000000000000000a040000000000000a040000000000000a0400000000000d49f40000000000016a0400000000000d49f40000000000016a040000000000000a040000000000000a040000000000000a04005000000000000000004a0400000000000f89f4000000000000aa0400000000000f89f4000000000000aa0400000000000ec9f40000000000004a0400000000000ec9f40000000000004a0400000000000f89f40"
            ),
            Geometry::Polygon(Polygon::new(
                line(&[
                    (2048, 2048),
                    (2048, 2037),
                    (2059, 2037),
                    (2059, 2048),
                    (2048, 2048),
                ]),
                vec![line(&[
                    (2050, 2046),
                    (2053, 2046),
                    (2053, 2043),
                    (2050, 2043),
                    (2050, 2046),
                ])],
            ))
        );
    }

    #[test]
    fn coordinates_are_rounded() {
        assert_eq!(
            parse("0101000000cdcccccccccc00403333333333330bc0"),
            Geometry::Point(Point(c(2, -3)))
        );
    }

    #[rstest]
    #[case::multipoint(
        "010400000001000000",
        "0101000000000000000000f03f0000000000000040",
        Geometry::Point(Point(Coord { x: 1, y: 2 }))
    )]
    #[case::multilinestring(
        "010500000001000000",
        "01020000000200000000000000000000000000000000000000000000000000f03f000000000000f03f",
        Geometry::LineString(LineString(vec![Coord { x: 0, y: 0 }, Coord { x: 1, y: 1 }]))
    )]
    #[case::multipolygon(
        "010600000001000000",
        "0103000000010000000400000000000000000000000000000000000000000000000000f03f00000000000000000000000000
00f03f000000000000f03f00000000000000000000000000000000",
        Geometry::Polygon(Polygon::new(
            LineString(vec![
                Coord { x: 0, y: 0 },
                Coord { x: 1, y: 0 },
                Coord { x: 1, y: 1 },
                Coord { x: 0, y: 0 },
            ]),
            Vec::new(),
        ))
    )]
    fn a_single_part_multi_geometry_collapses_the_way_mvt_encodes_it(
        #[case] header: &str,
        #[case] part: &str,
        #[case] expected: Geometry<i32>,
    ) {
        let hex = format!("{header}{}", part.replace('\n', ""));
        assert_eq!(parse(&hex), expected);
    }

    #[test]
    fn coordinate_beyond_i32_is_rejected() {
        assert_eq!(
            err("0101000000000000c00b5ae641000000000000f03f"),
            TileWkbError::CoordinateOutOfRange(3_000_000_000.0)
        );
    }

    #[test]
    fn empty_point_is_rejected() {
        assert_eq!(
            err("0101000000000000000000f87f000000000000f87f"),
            TileWkbError::EmptyPoint
        );
    }

    #[rstest]
    #[case::collection("010700000000000000")]
    #[case::collection_with_a_point("0107000000010000000101000000000000000000f03f0000000000000040")]
    fn geometry_collections_are_rejected(#[case] hex: &str) {
        assert_eq!(
            err(hex),
            TileWkbError::UnsupportedGeometry("GeometryCollection")
        );
    }

    /// `Wkb::try_new` builds the whole nested tree before naming its type, so rejecting
    /// `GeometryCollection` in [`Reader::geometry`] comes too late to stop the recursion.
    /// Same upstream fix as [`hostile_element_counts_are_rejected`].
    #[test]
    #[ignore = "overflows the stack: unbounded recursion in wkb 0.9.2"]
    fn deeply_nested_collections_are_rejected_without_overflowing_the_stack() {
        let mut bytes = Vec::new();
        for _ in 0..10_000 {
            bytes.extend_from_slice(&wkb("010700000001000000"));
        }
        bytes.extend_from_slice(&wkb("010100000000000000000024400000000000003440"));
        assert_eq!(
            parse_tile_wkb(&bytes),
            Err(TileWkbError::UnsupportedGeometry("GeometryCollection"))
        );
    }

    #[rstest]
    #[case::no_bytes("")]
    #[case::order_only("01")]
    #[case::truncated_type("010100")]
    #[case::truncated_point("0101000000000000000000244000000000")]
    #[case::truncated_srid("0101000020110f00")]
    #[case::truncated_ring_count("01030000000200000005000000")]
    #[case::huge_vertex_count("0102000000ffffffff")]
    #[case::huge_point_count("0104000000ffffffff")]
    #[case::bad_byte_order("020100000000000000000024400000000000003440")]
    #[case::unknown_type_code("010800000000000000")]
    #[case::zero_type_code("010000000000000000")]
    #[case::trailing_bytes("010100000000000000000024400000000000003440ff")]
    #[case::trailing_geometry(
        "010100000000000000000024400000000000003440010100000000000000000024400000000000003440"
    )]
    fn malformed_input_is_rejected(#[case] hex: &str) {
        err(hex);
    }

    /// `wkb` 0.9.2 sizes a `Vec` from a ring or part count without checking it against the
    /// buffer, so each of these aborts the process on a failed multi-gigabyte allocation instead
    /// of returning an error. Fixed upstream by georust/wkb#93, which is unreleased: 0.9.3 is
    /// changelogged but not published to crates.io.
    #[test]
    #[ignore = "aborts the process: unbounded pre-allocation in wkb 0.9.2"]
    fn hostile_element_counts_are_rejected() {
        for hex in [
            "0103000000ffffffff",
            "0105000000ffffffff",
            "0106000000ffffffff",
        ] {
            err(hex);
        }
    }
}

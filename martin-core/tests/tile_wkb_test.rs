#![cfg(feature = "postgres")]

use martin_core::tiles::postgres::{TileWkbError, parse_tile_wkb};
use mlt_core::geo_types::{
    Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
};
use rstest::rstest;

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
    "0103000000010000000400000000000000000000000000000000000000000000000000f03f0000000000000000000000000000f03f000000000000f03f00000000000000000000000000000000",
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
    assert_eq!(parse(&format!("{header}{part}")), expected);
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

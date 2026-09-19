//! Reader for `PostGIS` WKB/EWKB geometries that already live in MVT tile coordinate space.
//!
//! `ST_AsMVTGeom` returns geometry in tile space but keeps any M ordinate, while `ST_AsMVT`
//! discards it. Serving MLT directly from `PostgreSQL` therefore skips `ST_AsMVT` and reads the
//! output of `ST_AsBinary(ST_AsMVTGeom(...))` here instead.

const SRID_FLAG: u32 = 0x2000_0000;
const Z_FLAG: u32 = 0x8000_0000;
const M_FLAG: u32 = 0x4000_0000;
const TYPE_MASK: u32 = 0x0FFF_FFFF;

const MAX_NESTING_DEPTH: usize = 32;

const MIN_GEOMETRY_BYTES: usize = 5;

/// Errors that can occur while reading tile-space WKB.
#[non_exhaustive]
#[derive(thiserror::Error, Debug, Clone, PartialEq)]
pub enum TileWkbError {
    /// The buffer ended before the geometry did.
    #[error("WKB ended after {available} of the {needed} bytes needed at offset {offset}")]
    UnexpectedEndOfInput {
        /// Offset at which the read was attempted.
        offset: usize,
        /// Number of bytes the read wanted.
        needed: usize,
        /// Number of bytes actually left in the buffer.
        available: usize,
    },

    /// The byte order marker is neither `0x00` nor `0x01`.
    #[error("Unknown WKB byte order {0:#04x} at offset {1}, expected 0x00 or 0x01")]
    UnknownByteOrder(u8, usize),

    /// The type word does not name a geometry this reader knows.
    #[error("Unknown WKB geometry type {0:#010x} at offset {1}")]
    UnknownGeometryType(u32, usize),

    /// A multi-geometry contained a part of the wrong type.
    #[error("A {container} cannot contain a {part} at offset {offset}")]
    MismatchedPart {
        /// The multi-geometry being read.
        container: &'static str,
        /// The type of the offending part.
        part: &'static str,
        /// Offset at which the part ended.
        offset: usize,
    },

    /// A coordinate is not a tile coordinate: not finite, or outside [`i32`].
    #[error("Coordinate {value} at offset {offset} does not fit into an i32 tile coordinate")]
    CoordinateOutOfRange {
        /// The offending ordinate.
        value: f64,
        /// Offset at which it was read.
        offset: usize,
    },

    /// Geometry collections are nested deeper than this reader accepts.
    #[error("WKB is nested deeper than the supported {0} levels")]
    NestingTooDeep(usize),

    /// The geometry ended before the buffer did.
    #[error("{0} trailing bytes after the end of the WKB geometry")]
    TrailingBytes(usize),
}

/// A vertex in MVT tile coordinate space, carrying the measure `PostGIS` kept on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileVertex {
    /// Tile-space X ordinate.
    pub x: i32,
    /// Tile-space Y ordinate.
    pub y: i32,
    /// The M ordinate, if the geometry had one.
    pub m: Option<f64>,
}

/// A geometry in MVT tile coordinate space.
#[derive(Debug, Clone, PartialEq)]
pub enum TileGeometry {
    /// A single vertex.
    Point(TileVertex),
    /// An open or closed chain of vertices.
    LineString(Vec<TileVertex>),
    /// An exterior ring followed by any interior rings.
    Polygon(Vec<Vec<TileVertex>>),
    /// A set of vertices.
    MultiPoint(Vec<TileVertex>),
    /// A set of chains.
    MultiLineString(Vec<Vec<TileVertex>>),
    /// A set of polygons, each a list of rings.
    MultiPolygon(Vec<Vec<Vec<TileVertex>>>),
    /// A heterogeneous set of geometries.
    GeometryCollection(Vec<Self>),
}

fn type_name(base_type: u32) -> &'static str {
    match base_type {
        1 => "Point",
        2 => "LineString",
        3 => "Polygon",
        4 => "MultiPoint",
        5 => "MultiLineString",
        6 => "MultiPolygon",
        _ => "GeometryCollection",
    }
}

/// Reads one `PostGIS` WKB or EWKB geometry whose coordinates are already in tile space.
///
/// Z ordinates are discarded, M ordinates are kept on every vertex. The whole buffer must be
/// consumed by exactly one geometry.
pub fn parse_tile_wkb(bytes: &[u8]) -> Result<TileGeometry, TileWkbError> {
    let mut cursor = Cursor::new(bytes);
    let geometry = cursor.read_geometry(0)?;
    let trailing = cursor.remaining();
    if trailing > 0 {
        return Err(TileWkbError::TrailingBytes(trailing));
    }
    Ok(geometry)
}

#[derive(Clone, Copy)]
enum ByteOrder {
    Little,
    Big,
}

#[derive(Clone, Copy)]
struct Dimensions {
    has_z: bool,
    has_m: bool,
}

impl Dimensions {
    fn ordinate_count(self) -> usize {
        2 + usize::from(self.has_z) + usize::from(self.has_m)
    }
}

#[derive(Clone, Copy)]
struct Header {
    order: ByteOrder,
    base_type: u32,
    dims: Dimensions,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], TileWkbError> {
        let slice = self
            .offset
            .checked_add(len)
            .and_then(|end| self.bytes.get(self.offset..end))
            .ok_or(TileWkbError::UnexpectedEndOfInput {
                offset: self.offset,
                needed: len,
                available: self.remaining(),
            })?;
        self.offset += len;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8, TileWkbError> {
        Ok(self.take(1)?[0])
    }

    fn read_u32(&mut self, order: ByteOrder) -> Result<u32, TileWkbError> {
        let raw: [u8; 4] = self
            .take(4)?
            .try_into()
            .expect("take(4) always yields 4 bytes");
        Ok(match order {
            ByteOrder::Little => u32::from_le_bytes(raw),
            ByteOrder::Big => u32::from_be_bytes(raw),
        })
    }

    fn read_f64(&mut self, order: ByteOrder) -> Result<f64, TileWkbError> {
        let raw: [u8; 8] = self
            .take(8)?
            .try_into()
            .expect("take(8) always yields 8 bytes");
        Ok(match order {
            ByteOrder::Little => f64::from_le_bytes(raw),
            ByteOrder::Big => f64::from_be_bytes(raw),
        })
    }

    fn capacity_for(&self, count: u32, min_item_bytes: usize) -> usize {
        let affordable = self.remaining() / min_item_bytes;
        usize::try_from(count).unwrap_or(usize::MAX).min(affordable)
    }

    fn read_header(&mut self) -> Result<Header, TileWkbError> {
        let order_offset = self.offset;
        let order = match self.read_u8()? {
            1 => ByteOrder::Little,
            0 => ByteOrder::Big,
            other => return Err(TileWkbError::UnknownByteOrder(other, order_offset)),
        };

        let type_offset = self.offset;
        let raw_type = self.read_u32(order)?;
        let mut has_z = raw_type & Z_FLAG != 0;
        let mut has_m = raw_type & M_FLAG != 0;

        let mut base_type = raw_type & TYPE_MASK;
        match base_type / 1000 {
            0 => {}
            1 => has_z = true,
            2 => has_m = true,
            3 => {
                has_z = true;
                has_m = true;
            }
            _ => return Err(TileWkbError::UnknownGeometryType(raw_type, type_offset)),
        }
        base_type %= 1000;
        if !(1..=7).contains(&base_type) {
            return Err(TileWkbError::UnknownGeometryType(raw_type, type_offset));
        }

        if raw_type & SRID_FLAG != 0 {
            self.take(4)?;
        }

        Ok(Header {
            order,
            base_type,
            dims: Dimensions { has_z, has_m },
        })
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "the rounded value is bounds-checked against i32 before the cast"
    )]
    fn read_ordinate(&mut self, order: ByteOrder) -> Result<i32, TileWkbError> {
        let offset = self.offset;
        let value = self.read_f64(order)?;
        let rounded = value.round();
        if !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&rounded) {
            return Err(TileWkbError::CoordinateOutOfRange { value, offset });
        }
        Ok(rounded as i32)
    }

    fn read_vertex(
        &mut self,
        order: ByteOrder,
        dims: Dimensions,
    ) -> Result<TileVertex, TileWkbError> {
        let x = self.read_ordinate(order)?;
        let y = self.read_ordinate(order)?;
        if dims.has_z {
            self.read_f64(order)?;
        }
        let m = if dims.has_m {
            Some(self.read_f64(order)?)
        } else {
            None
        };
        Ok(TileVertex { x, y, m })
    }

    fn read_vertices(
        &mut self,
        order: ByteOrder,
        dims: Dimensions,
    ) -> Result<Vec<TileVertex>, TileWkbError> {
        let count = self.read_u32(order)?;
        let mut vertices = Vec::with_capacity(self.capacity_for(count, dims.ordinate_count() * 8));
        for _ in 0..count {
            vertices.push(self.read_vertex(order, dims)?);
        }
        Ok(vertices)
    }

    fn read_rings(
        &mut self,
        order: ByteOrder,
        dims: Dimensions,
    ) -> Result<Vec<Vec<TileVertex>>, TileWkbError> {
        let count = self.read_u32(order)?;
        let mut rings = Vec::with_capacity(self.capacity_for(count, 4));
        for _ in 0..count {
            rings.push(self.read_vertices(order, dims)?);
        }
        Ok(rings)
    }

    fn read_part_header(
        &mut self,
        expected: u32,
        container: &'static str,
    ) -> Result<Header, TileWkbError> {
        let offset = self.offset;
        let header = self.read_header()?;
        if header.base_type == expected {
            Ok(header)
        } else {
            Err(TileWkbError::MismatchedPart {
                container,
                part: type_name(header.base_type),
                offset,
            })
        }
    }

    fn read_geometry(&mut self, depth: usize) -> Result<TileGeometry, TileWkbError> {
        if depth > MAX_NESTING_DEPTH {
            return Err(TileWkbError::NestingTooDeep(MAX_NESTING_DEPTH));
        }
        let Header {
            order,
            base_type,
            dims,
        } = self.read_header()?;

        match base_type {
            1 => Ok(TileGeometry::Point(self.read_vertex(order, dims)?)),
            2 => Ok(TileGeometry::LineString(self.read_vertices(order, dims)?)),
            3 => Ok(TileGeometry::Polygon(self.read_rings(order, dims)?)),
            4 => {
                let count = self.read_u32(order)?;
                let mut vertices = Vec::with_capacity(self.capacity_for(count, MIN_GEOMETRY_BYTES));
                for _ in 0..count {
                    let part = self.read_part_header(1, "MultiPoint")?;
                    vertices.push(self.read_vertex(part.order, part.dims)?);
                }
                Ok(TileGeometry::MultiPoint(vertices))
            }
            5 => {
                let count = self.read_u32(order)?;
                let mut lines = Vec::with_capacity(self.capacity_for(count, MIN_GEOMETRY_BYTES));
                for _ in 0..count {
                    let part = self.read_part_header(2, "MultiLineString")?;
                    lines.push(self.read_vertices(part.order, part.dims)?);
                }
                Ok(TileGeometry::MultiLineString(lines))
            }
            6 => {
                let count = self.read_u32(order)?;
                let mut polygons = Vec::with_capacity(self.capacity_for(count, MIN_GEOMETRY_BYTES));
                for _ in 0..count {
                    let part = self.read_part_header(3, "MultiPolygon")?;
                    polygons.push(self.read_rings(part.order, part.dims)?);
                }
                Ok(TileGeometry::MultiPolygon(polygons))
            }
            _ => {
                let count = self.read_u32(order)?;
                let mut parts = Vec::with_capacity(self.capacity_for(count, MIN_GEOMETRY_BYTES));
                for _ in 0..count {
                    parts.push(self.read_geometry(depth + 1)?);
                }
                Ok(TileGeometry::GeometryCollection(parts))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn wkb(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .chunks(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                    .expect("fixture is valid hex")
            })
            .collect()
    }

    fn parse(hex: &str) -> TileGeometry {
        parse_tile_wkb(&wkb(hex)).expect("fixture should parse")
    }

    fn err(hex: &str) -> TileWkbError {
        parse_tile_wkb(&wkb(hex)).expect_err("fixture should not parse")
    }

    fn v(x: i32, y: i32) -> TileVertex {
        TileVertex { x, y, m: None }
    }

    fn vm(x: i32, y: i32, m: f64) -> TileVertex {
        TileVertex { x, y, m: Some(m) }
    }

    #[test]
    fn point() {
        assert_eq!(
            parse("010100000000000000000024400000000000003440"),
            TileGeometry::Point(v(10, 20))
        );
    }

    #[test]
    fn point_with_negative_coordinates() {
        assert_eq!(
            parse("010100000000000000000050c000000000000050c0"),
            TileGeometry::Point(v(-64, -64))
        );
    }

    #[test]
    fn linestring() {
        assert_eq!(
            parse(
                "010200000003000000000000000000000000000000000000000000000000002440000000000000244000000000000034400000000000001440"
            ),
            TileGeometry::LineString(vec![v(0, 0), v(10, 10), v(20, 5)])
        );
    }

    #[test]
    fn polygon_with_interior_ring() {
        assert_eq!(
            parse(
                "010300000002000000050000000000000000000000000000000000000000000000000059400000000000000000000000000000594000000000000059400000000000000000000000000000594000000000000000000000000000000000050000000000000000002440000000000000244000000000000034400000000000002440000000000000344000000000000034400000000000002440000000000000344000000000000024400000000000002440"
            ),
            TileGeometry::Polygon(vec![
                vec![v(0, 0), v(100, 0), v(100, 100), v(0, 100), v(0, 0)],
                vec![v(10, 10), v(20, 10), v(20, 20), v(10, 20), v(10, 10)],
            ])
        );
    }

    #[test]
    fn multipoint() {
        assert_eq!(
            parse(
                "0104000000020000000101000000000000000000f03f0000000000000040010100000000000000000008400000000000001040"
            ),
            TileGeometry::MultiPoint(vec![v(1, 2), v(3, 4)])
        );
    }

    #[test]
    fn multilinestring() {
        assert_eq!(
            parse(
                "01050000000200000001020000000200000000000000000000000000000000000000000000000000f03f000000000000f03f010200000003000000000000000000004000000000000000400000000000000840000000000000084000000000000010400000000000001040"
            ),
            TileGeometry::MultiLineString(vec![
                vec![v(0, 0), v(1, 1)],
                vec![v(2, 2), v(3, 3), v(4, 4)],
            ])
        );
    }

    #[test]
    fn multipolygon() {
        assert_eq!(
            parse(
                "010600000002000000010300000001000000040000000000000000000000000000000000000000000000000024400000000000000000000000000000244000000000000024400000000000000000000000000000000001030000000100000004000000000000000000344000000000000034400000000000003e4000000000000034400000000000003e400000000000003e4000000000000034400000000000003440"
            ),
            TileGeometry::MultiPolygon(vec![
                vec![vec![v(0, 0), v(10, 0), v(10, 10), v(0, 0)]],
                vec![vec![v(20, 20), v(30, 20), v(30, 30), v(20, 20)]],
            ])
        );
    }

    #[test]
    fn geometry_collection() {
        assert_eq!(
            parse(
                "0107000000020000000101000000000000000000f03f00000000000000400102000000020000000000000000000840000000000000104000000000000014400000000000001840"
            ),
            TileGeometry::GeometryCollection(vec![
                TileGeometry::Point(v(1, 2)),
                TileGeometry::LineString(vec![v(3, 4), v(5, 6)]),
            ])
        );
    }

    #[test]
    fn nested_geometry_collection() {
        assert_eq!(
            parse(
                "0107000000020000000107000000010000000101000000000000000000f03f00000000000000400103000000010000000400000000000000000000000000000000000000000000000000f03f0000000000000000000000000000f03f000000000000f03f00000000000000000000000000000000"
            ),
            TileGeometry::GeometryCollection(vec![
                TileGeometry::GeometryCollection(vec![TileGeometry::Point(v(1, 2))]),
                TileGeometry::Polygon(vec![vec![v(0, 0), v(1, 0), v(1, 1), v(0, 0)]]),
            ])
        );
    }

    #[test]
    fn xym_point_keeps_measure() {
        assert_eq!(
            parse("01d1070000000000000000084000000000000010400000000000001c40"),
            TileGeometry::Point(vm(3, 4, 7.0))
        );
    }

    #[test]
    fn xym_linestring_keeps_measure() {
        assert_eq!(
            parse(
                "01d207000002000000000000000000000000000000000000000000000000002440000000000000f03f000000000000f03f0000000000003440"
            ),
            TileGeometry::LineString(vec![vm(0, 0, 10.0), vm(1, 1, 20.0)])
        );
    }

    #[test]
    fn xyzm_linestring_drops_z_keeps_measure() {
        assert_eq!(
            parse(
                "01ba0b0000020000000000000000000000000000000000000000000000000014400000000000002440000000000000f03f000000000000f03f00000000000018400000000000003440"
            ),
            TileGeometry::LineString(vec![vm(0, 0, 10.0), vm(1, 1, 20.0)])
        );
    }

    #[test]
    fn xyz_linestring_drops_z_without_measure() {
        assert_eq!(
            parse(
                "01ea03000002000000000000000000000000000000000000000000000000001440000000000000f03f000000000000f03f0000000000001840"
            ),
            TileGeometry::LineString(vec![v(0, 0), v(1, 1)])
        );
    }

    #[test]
    fn xym_multipoint_keeps_measure() {
        assert_eq!(
            parse(
                "01d40700000200000001d1070000000000000000f03f0000000000000040000000000000084001d1070000000000000000104000000000000014400000000000001840"
            ),
            TileGeometry::MultiPoint(vec![vm(1, 2, 3.0), vm(4, 5, 6.0)])
        );
    }

    #[test]
    fn xym_multipolygon_keeps_measure() {
        assert_eq!(
            parse(
                "01d60700000100000001d3070000020000000400000000000000000000000000000000000000000000000000f03f00000000000010400000000000000000000000000000004000000000000010400000000000001040000000000000084000000000000000000000000000000000000000000000104004000000000000000000f03f000000000000f03f00000000000014400000000000000040000000000000f03f0000000000001840000000000000004000000000000000400000000000001c40000000000000f03f000000000000f03f0000000000002040"
            ),
            TileGeometry::MultiPolygon(vec![vec![
                vec![vm(0, 0, 1.0), vm(4, 0, 2.0), vm(4, 4, 3.0), vm(0, 0, 4.0),],
                vec![vm(1, 1, 5.0), vm(2, 1, 6.0), vm(2, 2, 7.0), vm(1, 1, 8.0),],
            ]])
        );
    }

    #[test]
    fn big_endian_point() {
        assert_eq!(
            parse("000000000140240000000000004034000000000000"),
            TileGeometry::Point(v(10, 20))
        );
    }

    #[test]
    fn big_endian_polygon() {
        assert_eq!(
            parse(
                "0000000003000000010000000400000000000000000000000000000000401000000000000000000000000000004010000000000000401000000000000000000000000000000000000000000000"
            ),
            TileGeometry::Polygon(vec![vec![v(0, 0), v(4, 0), v(4, 4), v(0, 0)]])
        );
    }

    #[test]
    fn big_endian_xym_linestring() {
        assert_eq!(
            parse(
                "00000007d2000000020000000000000000000000000000000040240000000000003ff00000000000003ff00000000000004034000000000000"
            ),
            TileGeometry::LineString(vec![vm(0, 0, 10.0), vm(1, 1, 20.0)])
        );
    }

    #[test]
    fn big_endian_geometry_collection() {
        assert_eq!(
            parse(
                "00000007d70000000200000007d13ff00000000000004000000000000000400800000000000000000007d2000000024008000000000000401000000000000040140000000000004018000000000000401c0000000000004020000000000000"
            ),
            TileGeometry::GeometryCollection(vec![
                TileGeometry::Point(vm(1, 2, 3.0)),
                TileGeometry::LineString(vec![vm(3, 4, 5.0), vm(6, 7, 8.0)]),
            ])
        );
    }

    #[test]
    fn byte_order_may_differ_per_nested_geometry() {
        assert_eq!(
            parse("010700000001000000000000000140240000000000000000000000000000"),
            TileGeometry::GeometryCollection(vec![TileGeometry::Point(v(10, 0))])
        );
    }

    #[test]
    fn ewkb_srid_is_skipped() {
        assert_eq!(
            parse("0101000020110f000000000000000024400000000000003440"),
            TileGeometry::Point(v(10, 20))
        );
    }

    #[test]
    fn ewkb_srid_and_measure_flags() {
        assert_eq!(
            parse(
                "0102000060110f000002000000000000000000000000000000000000000000000000002440000000000000f03f000000000000f03f0000000000003440"
            ),
            TileGeometry::LineString(vec![vm(0, 0, 10.0), vm(1, 1, 20.0)])
        );
    }

    #[test]
    fn ewkb_srid_with_zm_flags_drops_z() {
        assert_eq!(
            parse(
                "01030000e0e6100000010000000400000000000000000000000000000000000000000000000000f03f0000000000000040000000000000f03f000000000000000000000000000008400000000000001040000000000000f03f000000000000f03f00000000000014400000000000001840000000000000000000000000000000000000000000001c400000000000002040"
            ),
            TileGeometry::Polygon(vec![vec![
                vm(0, 0, 2.0),
                vm(1, 0, 4.0),
                vm(1, 1, 6.0),
                vm(0, 0, 8.0),
            ]])
        );
    }

    #[rstest]
    #[case::linestring("010200000000000000", TileGeometry::LineString(vec![]))]
    #[case::polygon("010300000000000000", TileGeometry::Polygon(vec![]))]
    #[case::multipoint("010400000000000000", TileGeometry::MultiPoint(vec![]))]
    #[case::geometry_collection("010700000000000000", TileGeometry::GeometryCollection(vec![]))]
    fn empty_geometries_parse(#[case] hex: &str, #[case] expected: TileGeometry) {
        assert_eq!(parse(hex), expected);
    }

    #[test]
    fn multilinestring_with_an_empty_part() {
        assert_eq!(
            parse(
                "010500000002000000010200000000000000010200000002000000000000000000f03f000000000000f03f00000000000000400000000000000040"
            ),
            TileGeometry::MultiLineString(vec![vec![], vec![v(1, 1), v(2, 2)]])
        );
    }

    #[test]
    fn st_asmvtgeom_linestring_keeps_measure() {
        assert_eq!(
            parse(
                "01d207000002000000000000000000a040000000000000a0400000000000002440000000000016a0400000000000d49f400000000000003440"
            ),
            TileGeometry::LineString(vec![vm(2048, 2048, 10.0), vm(2059, 2037, 20.0)])
        );
    }

    #[test]
    fn st_asmvtgeom_polygon_with_interior_ring() {
        assert_eq!(
            parse(
                "01030000000200000005000000000000000000a040000000000000a040000000000000a0400000000000d49f40000000000016a0400000000000d49f40000000000016a040000000000000a040000000000000a040000000000000a04005000000000000000004a0400000000000f89f4000000000000aa0400000000000f89f4000000000000aa0400000000000ec9f40000000000004a0400000000000ec9f40000000000004a0400000000000f89f40"
            ),
            TileGeometry::Polygon(vec![
                vec![
                    v(2048, 2048),
                    v(2048, 2037),
                    v(2059, 2037),
                    v(2059, 2048),
                    v(2048, 2048),
                ],
                vec![
                    v(2050, 2046),
                    v(2053, 2046),
                    v(2053, 2043),
                    v(2050, 2043),
                    v(2050, 2046),
                ],
            ])
        );
    }

    #[test]
    fn coordinates_are_rounded() {
        assert_eq!(
            parse("0101000000cdcccccccccc00403333333333330bc0"),
            TileGeometry::Point(v(2, -3))
        );
    }

    #[test]
    fn coordinate_beyond_i32_is_rejected() {
        assert!(matches!(
            err("0101000000000000c00b5ae641000000000000f03f"),
            TileWkbError::CoordinateOutOfRange { .. }
        ));
    }

    #[test]
    fn empty_point_is_rejected_because_it_is_nan() {
        assert!(matches!(
            err("0101000000000000000000f87f000000000000f87f"),
            TileWkbError::CoordinateOutOfRange { .. }
        ));
    }

    #[test]
    fn multipoint_containing_a_linestring_is_rejected() {
        assert!(matches!(
            err("010400000001000000010200000000000000"),
            TileWkbError::MismatchedPart {
                container: "MultiPoint",
                part: "LineString",
                ..
            }
        ));
    }

    #[rstest]
    #[case::no_bytes("")]
    #[case::order_only("01")]
    #[case::truncated_type("010100")]
    #[case::truncated_point("0101000000000000000000244000000000")]
    #[case::truncated_srid("0101000020110f00")]
    #[case::truncated_ring_count("01030000000200000005000000")]
    #[case::huge_vertex_count("0102000000ffffffff")]
    #[case::huge_ring_count("0103000000ffffffff")]
    #[case::huge_part_count("0107000000ffffffff")]
    #[case::bad_byte_order("020100000000000000000024400000000000003440")]
    #[case::bad_nested_byte_order("01070000000100000002010000000000000000002440")]
    #[case::unknown_type_code("010800000000000000")]
    #[case::unknown_iso_type_code("01a00f000000000000")]
    #[case::zero_type_code("010000000000000000")]
    #[case::trailing_bytes("010100000000000000000024400000000000003440ff")]
    #[case::trailing_geometry(
        "010100000000000000000024400000000000003440010100000000000000000024400000000000003440"
    )]
    fn malformed_input_is_rejected(#[case] hex: &str) {
        err(hex);
    }

    #[test]
    fn deep_nesting_is_rejected_without_overflowing_the_stack() {
        let mut bytes = Vec::new();
        for _ in 0..10_000 {
            bytes.extend_from_slice(&wkb("010700000001000000"));
        }
        bytes.extend_from_slice(&wkb("010100000000000000000024400000000000003440"));
        assert_eq!(
            parse_tile_wkb(&bytes),
            Err(TileWkbError::NestingTooDeep(MAX_NESTING_DEPTH))
        );
    }

    #[test]
    fn nesting_up_to_the_limit_is_accepted() {
        let mut bytes = Vec::new();
        for _ in 0..MAX_NESTING_DEPTH {
            bytes.extend_from_slice(&wkb("010700000001000000"));
        }
        bytes.extend_from_slice(&wkb("010100000000000000000024400000000000003440"));
        parse_tile_wkb(&bytes).expect("nesting at the limit must parse");
    }
}

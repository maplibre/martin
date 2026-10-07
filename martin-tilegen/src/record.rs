//! Temp format of one feature piece in one tile, every integer a varint:
//! `kind | id + 1 | properties byte length | property count | properties | geometry`.
//! Properties come first and carry their byte length, so the per-tile schema pass reads them without
//! touching vertices. Vertices are zigzag deltas from the previous vertex of the record, so tile-local
//! coordinates mostly take one or two bytes.

use integer_encoding::{VarInt, VarIntReader as _, VarIntWriter as _};

use crate::props::{KeyId, PropRef};
use crate::{TileGenError, TileGenResult};

pub type Vertex = [i32; 2];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeomKind {
    Point,
    Line,
    Polygon,
    /// The whole buffered tile is covered.
    Fill,
    /// Every tile from the record's own tile id up to `end` (exclusive) is covered.
    FillRange {
        end: u64,
    },
}

impl GeomKind {
    /// Pieces of one feature in one tile join into one multi-geometry only within a family.
    pub(crate) fn family(self) -> u8 {
        match self {
            Self::Point => 0,
            Self::Line => 1,
            Self::Polygon | Self::Fill | Self::FillRange { .. } => 2,
        }
    }
}

/// A piece of geometry to encode. Part lengths are vertex counts; rings are stored open.
#[derive(Clone, Copy, Debug)]
pub enum Geom<'a> {
    Points(&'a [Vertex]),
    Lines {
        parts: &'a [u32],
        vertices: &'a [Vertex],
    },
    /// `polygons` holds the ring count of each polygon (exterior first), `rings` the vertex count of each ring.
    Polygons {
        polygons: &'a [u32],
        rings: &'a [u32],
        vertices: &'a [Vertex],
    },
    Fill,
    FillRange {
        end: u64,
    },
}

const POINT: u8 = 0;
const LINE: u8 = 1;
const POLYGON: u8 = 2;
const FILL: u8 = 3;
const FILL_RANGE: u8 = 4;

const PROP_FALSE: u8 = 0;
const PROP_TRUE: u8 = 1;
const PROP_I64: u8 = 2;
const PROP_F32: u8 = 3;
const PROP_F64: u8 = 4;
const PROP_STR: u8 = 5;

/// A feature's properties, encoded once and copied into every record of the feature.
#[derive(Debug, Default)]
pub struct EncodedProps {
    count: u64,
    bytes: Vec<u8>,
}

impl EncodedProps {
    pub fn clear(&mut self) {
        self.count = 0;
        self.bytes.clear();
    }

    pub fn push(&mut self, key: KeyId, value: PropRef<'_>) {
        self.count += 1;
        write_prop(&mut self.bytes, key, value);
    }
}

pub fn encode(out: &mut Vec<u8>, id: Option<u64>, props: &EncodedProps, geom: Geom<'_>) {
    out.push(match geom {
        Geom::Points(_) => POINT,
        Geom::Lines { .. } => LINE,
        Geom::Polygons { .. } => POLYGON,
        Geom::Fill => FILL,
        Geom::FillRange { .. } => FILL_RANGE,
    });
    put(out, id.map_or(0, |id| id.wrapping_add(1)));
    put(out, props.bytes.len() as u64);
    put(out, props.count);
    out.extend_from_slice(&props.bytes);
    match geom {
        Geom::Points(vertices) => {
            put(out, vertices.len() as u64);
            write_vertices(out, vertices);
        }
        Geom::Lines { parts, vertices } => {
            debug_assert_eq!(
                parts.iter().map(|&n| n as usize).sum::<usize>(),
                vertices.len()
            );
            write_lengths(out, parts);
            write_vertices(out, vertices);
        }
        Geom::Polygons {
            polygons,
            rings,
            vertices,
        } => {
            debug_assert_eq!(
                polygons.iter().map(|&n| n as usize).sum::<usize>(),
                rings.len()
            );
            debug_assert_eq!(
                rings.iter().map(|&n| n as usize).sum::<usize>(),
                vertices.len()
            );
            write_lengths(out, polygons);
            for &n in rings {
                put(out, u64::from(n));
            }
            write_vertices(out, vertices);
        }
        Geom::Fill => {}
        Geom::FillRange { end } => put(out, end),
    }
}

fn write_prop(out: &mut Vec<u8>, key: KeyId, value: PropRef<'_>) {
    put(out, u64::from(key.0));
    match value {
        PropRef::Bool(v) => out.push(if v { PROP_TRUE } else { PROP_FALSE }),
        PropRef::I64(v) => {
            out.push(PROP_I64);
            put(out, v);
        }
        PropRef::F32(v) => {
            out.push(PROP_F32);
            out.extend_from_slice(&v.to_le_bytes());
        }
        PropRef::F64(v) => {
            out.push(PROP_F64);
            out.extend_from_slice(&v.to_le_bytes());
        }
        PropRef::Str(v) => {
            out.push(PROP_STR);
            put(out, v.len() as u64);
            out.extend_from_slice(v.as_bytes());
        }
    }
}

fn write_lengths(out: &mut Vec<u8>, lens: &[u32]) {
    put(out, lens.len() as u64);
    for &n in lens {
        put(out, u64::from(n));
    }
}

fn write_vertices(out: &mut Vec<u8>, vertices: &[Vertex]) {
    let mut prev = [0, 0];
    for &[x, y] in vertices {
        put(out, i64::from(x) - i64::from(prev[0]));
        put(out, i64::from(y) - i64::from(prev[1]));
        prev = [x, y];
    }
}

/// A record whose header is decoded; properties and geometry are decoded on demand.
pub struct Record<'a> {
    pub kind: GeomKind,
    pub id: Option<u64>,
    prop_count: u64,
    props: &'a [u8],
    geom: &'a [u8],
}

impl<'a> Record<'a> {
    pub fn decode(mut bytes: &'a [u8]) -> TileGenResult<Self> {
        let tag = take_byte(&mut bytes)?;
        let id = take::<u64>(&mut bytes)?.checked_sub(1);
        let props_len = take_len(&mut bytes)?;
        let prop_count = take::<u64>(&mut bytes)?;
        let (props, mut geom) = bytes
            .split_at_checked(props_len)
            .ok_or(TileGenError::CorruptRecord)?;
        let kind = match tag {
            POINT => GeomKind::Point,
            LINE => GeomKind::Line,
            POLYGON => GeomKind::Polygon,
            FILL => GeomKind::Fill,
            FILL_RANGE => GeomKind::FillRange {
                end: take::<u64>(&mut geom)?,
            },
            _ => return Err(TileGenError::CorruptRecord),
        };
        Ok(Self {
            kind,
            id,
            prop_count,
            props,
            geom,
        })
    }

    /// Whether an encoded record is a fill or fill range, without decoding it.
    #[must_use]
    pub fn is_fill(bytes: &[u8]) -> bool {
        matches!(bytes.first(), Some(&(FILL | FILL_RANGE)))
    }

    #[must_use]
    pub fn props(&self) -> Props<'a> {
        Props {
            bytes: self.props,
            remaining: self.prop_count,
        }
    }

    /// Appends the geometry, so the pieces of one feature can be collected into one multi-geometry.
    pub fn append_geometry(&self, out: &mut GeomBuf) -> TileGenResult<()> {
        let mut bytes = self.geom;
        let vertices = match self.kind {
            GeomKind::Point => take::<u64>(&mut bytes)?,
            GeomKind::Line => {
                let parts = take::<u64>(&mut bytes)?;
                take_lengths(&mut bytes, parts, &mut out.parts)?
            }
            GeomKind::Polygon => {
                let polygons = take::<u64>(&mut bytes)?;
                let rings = take_lengths(&mut bytes, polygons, &mut out.polygons)?;
                take_lengths(&mut bytes, rings, &mut out.parts)?
            }
            GeomKind::Fill | GeomKind::FillRange { .. } => 0,
        };
        out.vertices
            .reserve(usize::try_from(vertices).map_err(|_overflow| TileGenError::CorruptRecord)?);
        let mut prev = [0i64, 0];
        for _ in 0..vertices {
            prev[0] += take::<i64>(&mut bytes)?;
            prev[1] += take::<i64>(&mut bytes)?;
            let [Ok(x), Ok(y)] = prev.map(i32::try_from) else {
                return Err(TileGenError::CorruptRecord);
            };
            out.vertices.push([x, y]);
        }
        if bytes.is_empty() {
            Ok(())
        } else {
            Err(TileGenError::CorruptRecord)
        }
    }
}

/// Decoded geometry of one or more pieces of the same feature, shaped like [`Geom`].
#[derive(Debug, Default, PartialEq, Eq)]
pub struct GeomBuf {
    pub polygons: Vec<u32>,
    pub parts: Vec<u32>,
    pub vertices: Vec<Vertex>,
}

impl GeomBuf {
    pub fn clear(&mut self) {
        self.polygons.clear();
        self.parts.clear();
        self.vertices.clear();
    }
}

pub struct Props<'a> {
    bytes: &'a [u8],
    remaining: u64,
}

impl<'a> Iterator for Props<'a> {
    type Item = TileGenResult<(KeyId, PropRef<'a>)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        Some(read_prop(&mut self.bytes))
    }
}

fn read_prop<'a>(bytes: &mut &'a [u8]) -> TileGenResult<(KeyId, PropRef<'a>)> {
    let key =
        KeyId(u32::try_from(take::<u64>(bytes)?).map_err(|_overflow| TileGenError::CorruptRecord)?);
    let value = match take_byte(bytes)? {
        PROP_FALSE => PropRef::Bool(false),
        PROP_TRUE => PropRef::Bool(true),
        PROP_I64 => PropRef::I64(take::<i64>(bytes)?),
        PROP_F32 => PropRef::F32(f32::from_le_bytes(take_array(bytes)?)),
        PROP_F64 => PropRef::F64(f64::from_le_bytes(take_array(bytes)?)),
        PROP_STR => {
            let len = take_len(bytes)?;
            let (text, rest) = bytes
                .split_at_checked(len)
                .ok_or(TileGenError::CorruptRecord)?;
            *bytes = rest;
            PropRef::Str(std::str::from_utf8(text).map_err(|_invalid| TileGenError::CorruptRecord)?)
        }
        _ => return Err(TileGenError::CorruptRecord),
    };
    Ok((key, value))
}

/// Reads `count` lengths into `out`, returning their sum.
fn take_lengths(bytes: &mut &[u8], count: u64, out: &mut Vec<u32>) -> TileGenResult<u64> {
    let mut total = 0u64;
    for _ in 0..count {
        let len = take::<u64>(bytes)?;
        out.push(u32::try_from(len).map_err(|_overflow| TileGenError::CorruptRecord)?);
        total += len;
    }
    Ok(total)
}

fn put(out: &mut Vec<u8>, value: impl VarInt) {
    out.write_varint(value)
        .expect("writing to a Vec cannot fail");
}

fn take<T: VarInt>(bytes: &mut &[u8]) -> TileGenResult<T> {
    bytes
        .read_varint()
        .map_err(|_truncated| TileGenError::CorruptRecord)
}

fn take_len(bytes: &mut &[u8]) -> TileGenResult<usize> {
    take(bytes)
}

fn take_byte(bytes: &mut &[u8]) -> TileGenResult<u8> {
    let (&byte, rest) = bytes.split_first().ok_or(TileGenError::CorruptRecord)?;
    *bytes = rest;
    Ok(byte)
}

fn take_array<const N: usize>(bytes: &mut &[u8]) -> TileGenResult<[u8; N]> {
    let (array, rest) = bytes
        .split_first_chunk()
        .ok_or(TileGenError::CorruptRecord)?;
    *bytes = rest;
    Ok(*array)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROPS: [(KeyId, PropRef<'static>); 5] = [
        (KeyId(0), PropRef::Bool(true)),
        (KeyId(3), PropRef::I64(-42)),
        (KeyId(1), PropRef::F32(1.5)),
        (KeyId(2), PropRef::F64(-2.25)),
        (KeyId(9), PropRef::Str("héllo")),
    ];

    fn props() -> EncodedProps {
        let mut props = EncodedProps::default();
        for (key, value) in PROPS {
            props.push(key, value);
        }
        props
    }

    fn round_trip(geom: Geom<'_>, expected: &GeomBuf) {
        for id in [None, Some(0), Some(u64::MAX - 1)] {
            let mut bytes = Vec::new();
            encode(&mut bytes, id, &props(), geom);
            let record = Record::decode(&bytes).unwrap();
            assert_eq!(record.id, id);
            assert_eq!(
                record.props().collect::<TileGenResult<Vec<_>>>().unwrap(),
                PROPS
            );
            let mut buf = GeomBuf::default();
            record.append_geometry(&mut buf).unwrap();
            assert_eq!(&buf, expected);
        }
    }

    #[test]
    fn round_trips_every_kind() {
        let vertices = [[0, 0], [4096, -64], [i32::MIN, i32::MAX], [5, 5]];
        let buf = |polygons: &[u32], parts: &[u32]| GeomBuf {
            polygons: polygons.to_vec(),
            parts: parts.to_vec(),
            vertices: vertices.to_vec(),
        };
        round_trip(Geom::Points(&vertices), &buf(&[], &[]));
        round_trip(
            Geom::Lines {
                parts: &[1, 3],
                vertices: &vertices,
            },
            &buf(&[], &[1, 3]),
        );
        round_trip(
            Geom::Polygons {
                polygons: &[2],
                rings: &[3, 1],
                vertices: &vertices,
            },
            &buf(&[2], &[3, 1]),
        );
        round_trip(Geom::Fill, &GeomBuf::default());
        let mut bytes = Vec::new();
        encode(
            &mut bytes,
            None,
            &EncodedProps::default(),
            Geom::FillRange { end: 1 << 40 },
        );
        assert_eq!(
            Record::decode(&bytes).unwrap().kind,
            GeomKind::FillRange { end: 1 << 40 }
        );
    }

    #[test]
    fn pieces_append_into_one_buffer() {
        let mut buf = GeomBuf::default();
        for vertices in [&[[1, 1], [2, 2]][..], &[[3, 3], [4, 4], [5, 5]]] {
            let mut bytes = Vec::new();
            let parts = [u32::try_from(vertices.len()).unwrap()];
            encode(
                &mut bytes,
                Some(1),
                &EncodedProps::default(),
                Geom::Lines {
                    parts: &parts,
                    vertices,
                },
            );
            Record::decode(&bytes)
                .unwrap()
                .append_geometry(&mut buf)
                .unwrap();
        }
        assert_eq!(buf.parts, [2, 3]);
        assert_eq!(buf.vertices.len(), 5);
    }

    #[test]
    fn rejects_truncated_records() {
        let mut bytes = Vec::new();
        encode(
            &mut bytes,
            Some(7),
            &props(),
            Geom::Polygons {
                polygons: &[1],
                rings: &[2],
                vertices: &[[1, 2], [3, 4]],
            },
        );
        for len in 0..bytes.len() {
            let result = Record::decode(&bytes[..len]).and_then(|r| {
                r.props().try_for_each(|p| p.map(drop))?;
                r.append_geometry(&mut GeomBuf::default())
            });
            assert!(
                matches!(result, Err(TileGenError::CorruptRecord)),
                "len {len}"
            );
        }
    }
}

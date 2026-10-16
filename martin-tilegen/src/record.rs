//! Temp format of one feature piece in one tile, in [wincode] with varint integers: a [`Header`],
//! then the geometry of its kind. The header carries the properties as one length-prefixed blob, so
//! the per-tile schema pass reads them without touching vertices.

#![expect(
    clippy::mem_forget,
    reason = "the wincode SchemaRead derive forgets its drop guard"
)]

use crate::props::KeyId;
use mlt_core::PropValueRef;
use wincode::config::{Configuration, PREALLOCATION_SIZE_LIMIT_DISABLED};
use wincode::len::BincodeLen;
use wincode::{SchemaRead, SchemaWrite};

use crate::{TileGenError, TileGenResult};

pub type Vertex = [i32; 2];

pub(crate) type Config = Configuration<
    true,
    PREALLOCATION_SIZE_LIMIT_DISABLED,
    BincodeLen,
    wincode::int_encoding::LittleEndian,
    wincode::int_encoding::VarInt,
>;

pub(crate) const CONFIG: Config = Configuration::default()
    .with_varint_encoding()
    .disable_preallocation_size_limit();

#[derive(Clone, Copy, Debug, PartialEq, Eq, SchemaWrite, SchemaRead)]
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

#[derive(SchemaWrite, SchemaRead)]
enum Prop<'a> {
    Bool(bool),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    Str(&'a str),
}

#[derive(SchemaWrite, SchemaRead)]
struct Header<'a> {
    kind: GeomKind,
    id: Option<u64>,
    sort: Option<&'a [u8]>,
    props: &'a [u8],
}

/// A feature's properties, encoded once and copied into every record of the feature.
#[derive(Debug, Default)]
pub struct EncodedProps {
    bytes: Vec<u8>,
}

impl EncodedProps {
    pub fn clear(&mut self) {
        self.bytes.clear();
    }

    pub(crate) fn copy_from(&mut self, other: &Self) {
        self.bytes.clear();
        self.bytes.extend_from_slice(&other.bytes);
    }

    pub fn push(&mut self, key: KeyId, value: PropValueRef<'_>) {
        let value = match value {
            PropValueRef::Bool(v) => Prop::Bool(v),
            PropValueRef::I8(v) => Prop::I64(v.into()),
            PropValueRef::U8(v) => Prop::I64(v.into()),
            PropValueRef::I32(v) => Prop::I64(v.into()),
            PropValueRef::U32(v) => Prop::I64(v.into()),
            PropValueRef::I64(v) => Prop::I64(v),
            PropValueRef::U64(v) => Prop::U64(v),
            PropValueRef::F32(v) => Prop::F32(v),
            PropValueRef::F64(v) => Prop::F64(v),
            PropValueRef::Str(v) => Prop::Str(v),
        };
        put(&mut self.bytes, &(key.get(), value));
    }

    pub(crate) fn decode(&self) -> Props<'_> {
        Props { bytes: &self.bytes }
    }
}

/// `sort` is the feature's `sort_by` key, which orders the features of its layer in each tile.
pub fn encode(
    out: &mut Vec<u8>,
    id: Option<u64>,
    sort: Option<&[u8]>,
    props: &EncodedProps,
    geom: Geom<'_>,
) {
    let kind = match geom {
        Geom::Points(_) => GeomKind::Point,
        Geom::Lines { .. } => GeomKind::Line,
        Geom::Polygons { .. } => GeomKind::Polygon,
        Geom::Fill => GeomKind::Fill,
        Geom::FillRange { end } => GeomKind::FillRange { end },
    };
    let props = &props.bytes;
    put(
        out,
        &Header {
            kind,
            id,
            sort,
            props,
        },
    );
    match geom {
        Geom::Points(vertices) => put(out, vertices),
        Geom::Lines { parts, vertices } => put(out, &(parts, vertices)),
        Geom::Polygons {
            polygons,
            rings,
            vertices,
        } => put(out, &(polygons, rings, vertices)),
        Geom::Fill | Geom::FillRange { .. } => {}
    }
}

/// A record whose header is decoded; properties and geometry are decoded on demand.
pub struct Record<'a> {
    pub kind: GeomKind,
    pub id: Option<u64>,
    /// The feature's `sort_by` key, if its layer has one.
    pub sort: Option<&'a [u8]>,
    props: &'a [u8],
    geom: &'a [u8],
}

impl<'a> Record<'a> {
    pub fn decode(mut bytes: &'a [u8]) -> TileGenResult<Self> {
        let Header {
            kind,
            id,
            sort,
            props,
        } = take(&mut bytes)?;
        Ok(Self {
            kind,
            id,
            sort,
            props,
            geom: bytes,
        })
    }

    /// Whether an encoded record is a fill or fill range, without decoding the rest of it.
    #[must_use]
    pub fn is_fill(mut bytes: &[u8]) -> bool {
        matches!(
            take(&mut bytes),
            Ok(GeomKind::Fill | GeomKind::FillRange { .. })
        )
    }

    #[must_use]
    pub fn props(&self) -> Props<'a> {
        Props { bytes: self.props }
    }

    /// Appends the geometry, so the pieces of one feature can be collected into one multi-geometry.
    pub fn append_geometry(&self, out: &mut GeomBuf) -> TileGenResult<()> {
        let mut bytes = self.geom;
        let vertices: Vec<Vertex> = match self.kind {
            GeomKind::Point => take(&mut bytes)?,
            GeomKind::Line => {
                let (parts, vertices): (Vec<u32>, _) = take(&mut bytes)?;
                out.parts.extend(parts);
                vertices
            }
            GeomKind::Polygon => {
                let (polygons, rings, vertices): (Vec<u32>, Vec<u32>, _) = take(&mut bytes)?;
                out.polygons.extend(polygons);
                out.parts.extend(rings);
                vertices
            }
            GeomKind::Fill | GeomKind::FillRange { .. } => Vec::new(),
        };
        out.vertices.extend(vertices);
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
}

impl<'a> Iterator for Props<'a> {
    type Item = TileGenResult<(KeyId, PropValueRef<'a>)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.bytes.is_empty() {
            return None;
        }
        Some(take(&mut self.bytes).map(|(key, value)| {
            let value = match value {
                Prop::Bool(v) => PropValueRef::Bool(v),
                Prop::I64(v) => PropValueRef::I64(v),
                Prop::U64(v) => PropValueRef::U64(v),
                Prop::F32(v) => PropValueRef::F32(v),
                Prop::F64(v) => PropValueRef::F64(v),
                Prop::Str(v) => PropValueRef::Str(v),
            };
            (KeyId::new(key), value)
        }))
    }
}

fn put<T: SchemaWrite<Config, Src = T> + ?Sized>(out: &mut Vec<u8>, value: &T) {
    wincode::config::serialize_into(out, value, CONFIG).expect("writing to a Vec cannot fail");
}

fn take<'a, T: SchemaRead<'a, Config, Dst = T>>(bytes: &mut &'a [u8]) -> TileGenResult<T> {
    T::get(bytes).map_err(|_corrupt| TileGenError::CorruptRecord)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROPS: [(KeyId, PropValueRef<'static>); 6] = [
        (KeyId::new(0), PropValueRef::Bool(true)),
        (KeyId::new(3), PropValueRef::I64(-42)),
        (KeyId::new(4), PropValueRef::U64(u64::MAX)),
        (KeyId::new(1), PropValueRef::F32(1.5)),
        (KeyId::new(2), PropValueRef::F64(-2.25)),
        (KeyId::new(9), PropValueRef::Str("héllo")),
    ];

    fn props() -> EncodedProps {
        let mut props = EncodedProps::default();
        for (key, value) in PROPS {
            props.push(key, value);
        }
        props
    }

    fn round_trip(geom: Geom<'_>, expected: &GeomBuf) {
        let ids = [None, Some(0), Some(u64::MAX - 1)];
        let sorts = [None, Some(&[][..]), Some(&[0, 0xFF, 7][..])];
        for (id, sort) in ids.into_iter().flat_map(|id| sorts.map(|sort| (id, sort))) {
            let mut bytes = Vec::new();
            encode(&mut bytes, id, sort, &props(), geom);
            let record = Record::decode(&bytes).unwrap();
            assert_eq!(record.id, id);
            assert_eq!(record.sort, sort);
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
            Some(&[3]),
            &EncodedProps::default(),
            Geom::FillRange { end: 1 << 40 },
        );
        let record = Record::decode(&bytes).unwrap();
        assert_eq!(record.kind, GeomKind::FillRange { end: 1 << 40 });
        assert_eq!(record.sort, Some(&[3][..]));
        assert!(Record::is_fill(&bytes));
    }

    #[test]
    fn the_sort_key_is_an_optional_field_after_the_id() {
        let mut props = EncodedProps::default();
        props.push(KeyId::new(2), PropValueRef::I64(-1));
        let geom = Geom::Points(&[[3, -2]]);
        let mut unsorted = Vec::new();
        encode(&mut unsorted, Some(4), None, &props, geom);
        assert_eq!(unsorted, [0, 1, 4, 0, 3, 2, 1, 1, 1, 6, 3]);
        let mut sorted = Vec::new();
        encode(&mut sorted, Some(4), Some(&[9, 8]), &props, geom);
        assert_eq!(sorted, [0, 1, 4, 1, 2, 9, 8, 3, 2, 1, 1, 1, 6, 3]);
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
                None,
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
            Some(&[1, 2]),
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

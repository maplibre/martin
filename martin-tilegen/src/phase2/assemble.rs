//! Builds one layer of one tile from the records the merge yields for it.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::ops::Range;

use mlt_core::geo_types::Coord;
use mlt_core::{FeatureWriter, GeometryType, LayerWriter, MltResult, PropKind, PropValue, PropValueRef, PropertyKey};

use super::columns::TileColumns;
use super::per_tile::{LayerOpCounts, LayerTile, OpScratch, TileFeature, TileOps};
use crate::props::{KeyId, KeyNames};
use crate::sort::record::{GeomBuf, GeomKind, Record, Vertex};
use crate::sort::{LayerId, Seq};
use crate::{LayerGrid, TileGenResult};

/// Where a record of an output layer comes from: the layer definition that rendered it, which also sets the buffer
/// its fill covers, and the feature it is a piece of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecordOrigin {
    /// The layer byte of the definition's sort key.
    pub layer: LayerId,
    /// The feature's position in its source.
    pub seq: Seq,
    /// Tile units around the tile that a fill covers.
    pub fill_buffer: u32,
}

/// Reusable per-worker scratch for [`LayerAssembler::assemble`].
#[derive(Default)]
pub struct LayerAssembler {
    columns: TileColumns,
    schema: Vec<(KeyId, PropKind)>,
    geom: GeomBuf,
    /// Each feature's records and properties, as ranges of the decoded records and properties.
    features: Vec<TileFeature>,
    keys: Vec<PropertyKey>,
    text: String,
    texts: Vec<Range<usize>>,
    /// The integers of the last layer that float columns rounded.
    rounded: Rounded,
    /// Scratch of the per-tile operations, and their failed evaluations.
    pub(crate) ops: OpScratch,
}

/// A layer [`LayerAssembler::assemble`] built, borrowing its records, names and the assembler.
pub struct AssembledLayer<'a> {
    pub layer: LayerWriter<'a>,
    /// `(key, kind)` per property column, in column order.
    pub columns: &'a [(KeyId, PropKind)],
    /// What the per-tile operations did to the layer.
    pub(crate) ops: LayerOpCounts,
    /// The integers that float columns rounded.
    pub(crate) rounded: &'a Rounded,
}

/// How many integers a float column rounded, by key, of the few keys that had any.
#[derive(Clone, Debug, Default)]
pub(crate) struct Rounded(Vec<(KeyId, u64)>);

impl Rounded {
    /// Adds `count` to `key`.
    pub(crate) fn add(&mut self, key: KeyId, count: u64) {
        if let Some((_, total)) = self.0.iter_mut().find(|(known, _)| *known == key) {
            *total += count;
        } else {
            self.0.push((key, count));
        }
    }

    /// Adds every key of `other`.
    pub(crate) fn merge(&mut self, other: &Self) {
        for &(key, count) in &other.0 {
            self.add(key, count);
        }
    }

    /// Whether no value was rounded.
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The keys with rounded values, in the order they first had one.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (KeyId, u64)> + '_ {
        self.0.iter().copied()
    }

    fn clear(&mut self) {
        self.0.clear();
    }
}

impl LayerAssembler {
    /// `records` are one output layer's records in one tile, by definition and in merge (seq) order within one.
    /// Consecutive records of the same origin are the pieces of one feature in this tile (multi-part geometry, or
    /// a wrapped antimeridian twin) and become one multi-geometry feature; their properties and sort keys are
    /// identical, so the first record's are used. Features with a sort key are then stably sorted by it, and `ops`
    /// run on them in that order. The `hidden` properties, of any definition, are left out once `ops` ran.
    pub fn assemble<'a>(
        &'a mut self,
        name: &'a str,
        grid: LayerGrid,
        names: &'a KeyNames,
        hidden: &[KeyId],
        records: &[(RecordOrigin, &'a [u8])],
        ops: TileOps<'_>,
    ) -> TileGenResult<AssembledLayer<'a>> {
        let Self {
            columns,
            schema,
            geom,
            features,
            keys,
            text,
            texts,
            rounded,
            ops: op_scratch,
        } = self;
        let decoded = records
            .iter()
            .map(|&(origin, bytes)| Ok((origin, Record::decode(bytes)?)))
            .collect::<TileGenResult<Vec<_>>>()?;

        let mut props = Vec::new();
        group_features(&decoded, &mut props, features)?;
        op_scratch.counts = LayerOpCounts::default();
        let tile = LayerTile {
            decoded: &decoded,
            props: RefCell::new(props),
            names,
            grid,
            merged: RefCell::default(),
        };
        ops.apply(&tile, features, op_scratch)?;
        let props = tile.props.borrow();

        columns.clear();
        for feature in features.iter() {
            for &(key, value) in &props[feature.props()] {
                if !hidden.contains(&key) {
                    columns.add(key, value);
                }
            }
        }
        columns.finish(names, schema);

        text.clear();
        texts.clear();
        for feature in features.iter() {
            for &(key, value) in &props[feature.props()] {
                if let Some(pos) = columns.position(key)
                    && schema[pos].1 == PropKind::Str
                    && !matches!(value, PropValueRef::Str(_))
                {
                    let from = text.len();
                    write_text(text, value);
                    texts.push(from..text.len());
                }
            }
        }
        let text = text.as_str();
        let mut texts = texts.iter();

        let mut layer = LayerWriter::new(name, grid.extent)?;
        rounded.clear();
        keys.clear();
        for &(key, kind) in schema.iter() {
            keys.push(layer.add_property(names.name(key), kind)?);
        }

        for tile_feature in features.iter() {
            let (kind, id) = tile.geometry(tile_feature, geom)?;
            let mut feature = layer.feature(geometry_type(kind, geom));
            feature.id(id);
            write_geometry(&mut feature, kind, geom)?;
            for &(key, value) in &props[tile_feature.props()] {
                let Some(pos) = columns.position(key) else {
                    continue;
                };
                let kind = schema[pos].1;
                let value = if value.kind() == kind {
                    Some(value)
                } else if kind == PropKind::Str {
                    texts
                        .next()
                        .and_then(|range| text.get(range.clone()))
                        .map(PropValueRef::Str)
                } else {
                    let coerced = scalar(&value.coerce(kind));
                    if is_rounded(value, coerced) {
                        rounded.add(key, 1);
                    }
                    coerced
                };
                if let Some(value) = value {
                    feature.property(keys[pos], value)?;
                }
            }
            feature.finish()?;
        }
        Ok(AssembledLayer {
            layer,
            columns: schema,
            ops: op_scratch.counts,
            rounded,
        })
    }
}

/// Groups consecutive records of one feature and family into `features`, with their properties in
/// `props`, then stably sorts the features by their sort key, if they have one.
fn group_features<'a>(
    decoded: &[(RecordOrigin, Record<'a>)],
    props: &mut Vec<(KeyId, PropValueRef<'a>)>,
    features: &mut Vec<TileFeature>,
) -> TileGenResult<()> {
    features.clear();
    let (mut start, mut sorted) = (0, false);
    while let Some((origin, first)) = decoded.get(start) {
        let len = decoded[start..]
            .iter()
            .take_while(|(o, r)| o == origin && r.kind.family() == first.kind.family())
            .count();
        let from = props.len();
        for prop in first.props() {
            props.push(prop?);
        }
        features.push(TileFeature::new(start..start + len, from..props.len()));
        start += len;
        sorted |= first.sort.is_some();
    }
    if sorted {
        features.sort_by(|a, b| decoded[a.first()].1.sort.cmp(&decoded[b.first()].1.sort));
    }
    Ok(())
}

fn write_text(text: &mut String, value: PropValueRef<'_>) {
    match value {
        PropValueRef::Bool(v) => write!(text, "{v}"),
        PropValueRef::I8(v) => write!(text, "{v}"),
        PropValueRef::U8(v) => write!(text, "{v}"),
        PropValueRef::I32(v) => write!(text, "{v}"),
        PropValueRef::U32(v) => write!(text, "{v}"),
        PropValueRef::I64(v) => write!(text, "{v}"),
        PropValueRef::U64(v) => write!(text, "{v}"),
        PropValueRef::F32(v) => write!(text, "{v}"),
        PropValueRef::F64(v) => write!(text, "{v}"),
        PropValueRef::Str(v) => write!(text, "{v}"),
    }
    .expect("writing to a String never fails");
}

/// Whether a float column rounded integer `value` to `coerced`.
fn is_rounded(value: PropValueRef<'_>, coerced: Option<PropValueRef<'_>>) -> bool {
    let int = match value {
        PropValueRef::I8(v) => i128::from(v),
        PropValueRef::U8(v) => i128::from(v),
        PropValueRef::I32(v) => i128::from(v),
        PropValueRef::U32(v) => i128::from(v),
        PropValueRef::I64(v) => i128::from(v),
        PropValueRef::U64(v) => i128::from(v),
        PropValueRef::Bool(_) | PropValueRef::F32(_) | PropValueRef::F64(_) | PropValueRef::Str(_) => return false,
    };
    #[expect(
        clippy::cast_possible_truncation,
        reason = "an exact float casts back to the same integer"
    )]
    let rounded = matches!(coerced, Some(PropValueRef::F64(float)) if float as i128 != int);
    rounded
}

/// A non-string value as written into a column, or [`None`] for a null.
fn scalar<'a>(value: &PropValue) -> Option<PropValueRef<'a>> {
    match *value {
        PropValue::Bool(v) => v.map(PropValueRef::Bool),
        PropValue::I8(v) => v.map(PropValueRef::I8),
        PropValue::U8(v) => v.map(PropValueRef::U8),
        PropValue::I32(v) => v.map(PropValueRef::I32),
        PropValue::U32(v) => v.map(PropValueRef::U32),
        PropValue::I64(v) => v.map(PropValueRef::I64),
        PropValue::U64(v) => v.map(PropValueRef::U64),
        PropValue::F32(v) => v.map(PropValueRef::F32),
        PropValue::F64(v) => v.map(PropValueRef::F64),
        PropValue::Str(_) => None,
    }
}

/// A single geometry unless the feature's pieces made several of them.
fn geometry_type(kind: GeomKind, geom: &GeomBuf) -> GeometryType {
    match kind {
        GeomKind::Point | GeomKind::GriddedPoint { .. } if geom.vertices.len() == 1 => GeometryType::Point,
        GeomKind::Point | GeomKind::GriddedPoint { .. } => GeometryType::MultiPoint,
        GeomKind::Line if geom.parts.len() == 1 => GeometryType::LineString,
        GeomKind::Line => GeometryType::MultiLineString,
        GeomKind::Polygon | GeomKind::Fill | GeomKind::FillRange { .. } if geom.polygons.len() == 1 => {
            GeometryType::Polygon
        }
        GeomKind::Polygon | GeomKind::Fill | GeomKind::FillRange { .. } => GeometryType::MultiPolygon,
    }
}

fn coords(part: &[Vertex]) -> impl Iterator<Item = Coord<i32>> + '_ {
    part.iter().map(|&[x, y]| Coord { x, y })
}

fn write_geometry(feature: &mut FeatureWriter<'_, '_>, kind: GeomKind, geom: &GeomBuf) -> MltResult<()> {
    let mut vertices = geom.vertices.as_slice();
    let mut parts = geom.parts.iter().map(|&len| {
        let (part, rest) = vertices.split_at(len as usize);
        vertices = rest;
        part
    });
    match kind {
        GeomKind::Point | GeomKind::GriddedPoint { .. } => {
            feature.points(coords(&geom.vertices))?;
        }
        GeomKind::Line => {
            for line in parts {
                feature.line(coords(line))?;
            }
        }
        GeomKind::Polygon | GeomKind::Fill | GeomKind::FillRange { .. } => {
            for &rings in &geom.polygons {
                let mut rings = parts.by_ref().take(rings as usize);
                if let Some(exterior) = rings.next() {
                    feature.exterior_ring(coords(exterior))?;
                }
                for hole in rings {
                    feature.hole(coords(hole))?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use mlt_core::encoder::{EncoderConfig, Network, Profile};
    use mlt_core::geo_types::{Geometry, LineString, MultiLineString, Point};
    use mlt_core::{Decoder, Parser, PropValue, TileLayer};

    use super::*;
    use crate::props::KeyInterner;
    use crate::sort::record::{EncodedProps, Geom, encode};

    const GRID: LayerGrid = LayerGrid {
        extent: 4096,
        buffer: 64,
    };

    fn record(id: Option<u64>, props: &[(&str, PropValueRef<'_>)], interner: &KeyInterner, geom: Geom<'_>) -> Vec<u8> {
        let mut encoded = EncodedProps::default();
        for &(name, value) in props {
            encoded.push(interner.intern(name), value);
        }
        let mut bytes = Vec::new();
        encode(&mut bytes, id, None, &encoded, geom);
        bytes
    }

    fn decode_mlt(bytes: &[u8]) -> TileLayer {
        let mut layers = Parser::default().parse_layers(bytes).unwrap();
        assert_eq!(layers.len(), 1);
        layers
            .pop()
            .unwrap()
            .into_tile(&mut Decoder::default())
            .unwrap()
            .unwrap()
    }

    #[test]
    fn round_trips_through_mlt() {
        let interner = KeyInterner::new(["name", "height"]);
        let seq = |row| Seq::new(0, row).unwrap();
        let records = [
            (
                seq(1),
                record(
                    Some(10),
                    &[("name", PropValueRef::Str("a")), ("height", PropValueRef::I64(3))],
                    &interner,
                    Geom::Points(&[[5, 6]]),
                ),
            ),
            (
                seq(2),
                record(
                    Some(11),
                    &[("name", PropValueRef::Str("b")), ("extra", PropValueRef::Bool(true))],
                    &interner,
                    Geom::Lines {
                        parts: &[2],
                        vertices: &[[0, 0], [10, 10]],
                    },
                ),
            ),
            (
                seq(2),
                record(
                    Some(11),
                    &[("name", PropValueRef::Str("b")), ("extra", PropValueRef::Bool(true))],
                    &interner,
                    Geom::Lines {
                        parts: &[2],
                        vertices: &[[20, 20], [30, 30]],
                    },
                ),
            ),
            (
                seq(3),
                record(None, &[("height", PropValueRef::I64(-1))], &interner, Geom::Fill),
            ),
        ];
        let names = interner.freeze();
        let records: Vec<_> = records
            .iter()
            .map(|(seq, b)| {
                let origin = RecordOrigin {
                    seq: *seq,
                    fill_buffer: GRID.buffer,
                    ..RecordOrigin::default()
                };
                (origin, b.as_slice())
            })
            .collect();
        let mut scratch = LayerAssembler::default();
        let assembled = scratch
            .assemble("roads", GRID, &names, &[], &records, TileOps::NONE)
            .unwrap();
        let decoded = decode_mlt(
            &assembled
                .layer
                .encode(EncoderConfig::from_profile(Profile::Web(Network::Offline)))
                .unwrap(),
        );

        assert_eq!(decoded.name(), "roads");
        assert_eq!(decoded.property_names(), ["name", "height", "extra"]);
        let features = decoded.features();
        assert_eq!(features.len(), 3);
        assert_eq!(features[0].id(), Some(10));
        assert_eq!(features[0].geometry(), &Geometry::Point(Point::new(5, 6)));
        assert_eq!(
            features[1].geometry(),
            &Geometry::MultiLineString(MultiLineString(vec![
                LineString::from(vec![(0, 0), (10, 10)]),
                LineString::from(vec![(20, 20), (30, 30)]),
            ]))
        );
        assert_eq!(features[1].properties()[2], PropValue::Bool(Some(true)));
        assert_eq!(features[2].properties()[1], PropValue::I32(Some(-1)));
        let Geometry::Polygon(square) = features[2].geometry() else {
            panic!("fill is a polygon")
        };
        assert_eq!(square.exterior().0.first(), Some(&Coord { x: -64, y: -64 }));
    }

    #[test]
    fn sort_keys_reorder_whole_features_and_keep_ties_in_seq_order() {
        let interner = KeyInterner::new::<&str>([]);
        let names = interner.freeze();
        let line = |id, sort: &[u8], x| {
            let mut bytes = Vec::new();
            encode(
                &mut bytes,
                Some(id),
                Some(sort),
                &EncodedProps::default(),
                Geom::Lines {
                    parts: &[2],
                    vertices: &[[x, 0], [x, 10]],
                },
            );
            bytes
        };
        let seq = |row| Seq::new(0, row).unwrap();
        let records = [
            (seq(1), line(1, &[2], 0)),
            (seq(2), line(2, &[1, 5], 10)),
            (seq(2), line(2, &[1, 5], 20)),
            (seq(3), line(3, &[1], 30)),
            (seq(4), line(4, &[1, 5], 40)),
        ];
        let records: Vec<_> = records
            .iter()
            .map(|(seq, b)| {
                let origin = RecordOrigin {
                    seq: *seq,
                    fill_buffer: GRID.buffer,
                    ..RecordOrigin::default()
                };
                (origin, b.as_slice())
            })
            .collect();
        let mut scratch = LayerAssembler::default();
        let assembled = scratch
            .assemble("roads", GRID, &names, &[], &records, TileOps::NONE)
            .unwrap();
        let decoded = decode_mlt(
            &assembled
                .layer
                .encode(EncoderConfig::from_profile(Profile::Web(Network::Offline)))
                .unwrap(),
        );
        let features: Vec<_> = decoded
            .features()
            .iter()
            .map(|f| (f.id(), f.geometry().clone()))
            .collect();
        let line = |x| LineString::from(vec![(x, 0), (x, 10)]);
        assert_eq!(
            features,
            [
                (Some(3), Geometry::LineString(line(30))),
                (
                    Some(2),
                    Geometry::MultiLineString(MultiLineString(vec![line(10), line(20)]))
                ),
                (Some(4), Geometry::LineString(line(40))),
                (Some(1), Geometry::LineString(line(0))),
            ]
        );
    }

    #[test]
    fn polygons_with_holes_and_multipolygons() {
        let interner = KeyInterner::new::<&str>([]);
        let square = |o: i32, s: i32| [[o, o], [o + s, o], [o + s, o + s], [o, o + s]];
        let vertices: Vec<_> = [square(0, 100), square(10, 10), square(200, 50)].concat();
        let bytes = record(
            None,
            &[],
            &interner,
            Geom::Polygons {
                polygons: &[2, 1],
                rings: &[4, 4, 4],
                vertices: &vertices,
            },
        );
        let names = interner.freeze();
        let mut scratch = LayerAssembler::default();
        let assembled = scratch
            .assemble(
                "water",
                GRID,
                &names,
                &[],
                &[(
                    RecordOrigin {
                        fill_buffer: GRID.buffer,
                        ..RecordOrigin::default()
                    },
                    &bytes,
                )],
                TileOps::NONE,
            )
            .unwrap();
        let decoded = decode_mlt(
            &assembled
                .layer
                .encode(EncoderConfig::from_profile(Profile::Web(Network::Offline)))
                .unwrap(),
        );
        let Geometry::MultiPolygon(multi) = decoded.features()[0].geometry() else {
            panic!("two polygons")
        };
        assert_eq!(multi.0.len(), 2);
        assert_eq!(multi.0[0].interiors().len(), 1);
        assert_eq!(multi.0[1].exterior().0.len(), 5, "geo_types closes rings");
    }

    #[test]
    fn mixed_values_become_text_in_a_string_column() {
        let interner = KeyInterner::new(["kind", "rank"]);
        let seq = |row| Seq::new(0, row).unwrap();
        let records = [
            (
                seq(1),
                record(
                    None,
                    &[("kind", PropValueRef::Str("a")), ("rank", PropValueRef::I64(1))],
                    &interner,
                    Geom::Points(&[[1, 1]]),
                ),
            ),
            (
                seq(2),
                record(
                    None,
                    &[("kind", PropValueRef::I64(7)), ("rank", PropValueRef::F64(2.5))],
                    &interner,
                    Geom::Points(&[[2, 2]]),
                ),
            ),
            (
                seq(3),
                record(
                    None,
                    &[("kind", PropValueRef::Bool(true))],
                    &interner,
                    Geom::Points(&[[3, 3]]),
                ),
            ),
        ];
        let names = interner.freeze();
        let records: Vec<_> = records
            .iter()
            .map(|(seq, b)| {
                let origin = RecordOrigin {
                    seq: *seq,
                    fill_buffer: GRID.buffer,
                    ..RecordOrigin::default()
                };
                (origin, b.as_slice())
            })
            .collect();
        let mut scratch = LayerAssembler::default();
        let assembled = scratch
            .assemble("pois", GRID, &names, &[], &records, TileOps::NONE)
            .unwrap();
        let decoded = decode_mlt(
            &assembled
                .layer
                .encode(EncoderConfig::from_profile(Profile::Web(Network::Offline)))
                .unwrap(),
        );

        let values: Vec<_> = decoded.features().iter().map(|f| f.properties().to_vec()).collect();
        assert_eq!(
            values,
            [
                [PropValue::Str(Some("a".into())), PropValue::F64(Some(1.0))],
                [PropValue::Str(Some("7".into())), PropValue::F64(Some(2.5))],
                [PropValue::Str(Some("true".into())), PropValue::F64(None)],
            ]
        );
    }
}

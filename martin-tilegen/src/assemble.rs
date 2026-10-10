//! Builds one layer of one tile from the records the merge yields for it.

use std::ops::Range;

use mlt_core::geo_types::Coord;
use mlt_core::{FeatureWriter, GeometryType, LayerWriter, MltResult, PropKind, PropertyKey};

use crate::props::{KeyId, KeyNames, TileColumns};
use crate::record::{GeomBuf, GeomKind, Record, Vertex};
use crate::{Seq, TileGenResult};

/// The tile-local square a fill record stands for, including the buffer.
#[derive(Clone, Copy, Debug)]
pub struct LayerGrid {
    pub extent: u32,
    pub buffer: u32,
}

/// Reusable per-worker scratch for [`LayerAssembler::assemble`]; its [`LayerWriter`] keeps its
/// buffers from layer to layer, so assembling stops allocating once it has held the largest layer.
#[derive(Default)]
pub struct LayerAssembler {
    columns: TileColumns,
    schema: Vec<(KeyId, PropKind)>,
    geom: GeomBuf,
    /// Each feature's records, as a range of the decoded records.
    features: Vec<Range<usize>>,
    keys: Vec<PropertyKey>,
    text: String,
    writer: Option<LayerWriter>,
}

/// A layer [`LayerAssembler::assemble`] built, valid until it assembles the next one.
pub struct AssembledLayer<'a> {
    pub layer: &'a LayerWriter,
    /// `(key, kind)` per property column, in column order.
    pub columns: &'a [(KeyId, PropKind)],
}

impl LayerAssembler {
    /// `records` are one layer's records in one tile, in merge (seq) order. Consecutive records of the same
    /// feature are its pieces in this tile (multi-part geometry, or a wrapped antimeridian twin) and become
    /// one multi-geometry feature; their properties and sort keys are identical, so the first record's are
    /// used. Features with a sort key are then stably sorted by it.
    pub fn assemble(
        &mut self,
        name: &str,
        grid: LayerGrid,
        names: &KeyNames,
        records: &[(Seq, &[u8])],
    ) -> TileGenResult<AssembledLayer<'_>> {
        let Self {
            columns,
            schema,
            geom,
            features,
            keys,
            text,
            writer,
        } = self;
        let decoded = records
            .iter()
            .map(|&(seq, bytes)| Ok((seq, Record::decode(bytes)?)))
            .collect::<TileGenResult<Vec<_>>>()?;

        columns.clear();
        for (_, record) in &decoded {
            for prop in record.props() {
                let (key, value) = prop?;
                columns.add(key, value);
            }
        }
        columns.finish(names, schema);
        let layer = if let Some(layer) = writer {
            layer.reset(name, grid.extent)?;
            layer
        } else {
            writer.insert(LayerWriter::new(name, grid.extent)?)
        };
        keys.clear();
        for &(key, kind) in schema.iter() {
            keys.push(layer.add_property(names.name(key), kind)?);
        }

        features.clear();
        let (mut start, mut sorted) = (0, false);
        while let Some((seq, first)) = decoded.get(start) {
            let len = decoded[start..]
                .iter()
                .take_while(|(s, r)| s == seq && r.kind.family() == first.kind.family())
                .count();
            features.push(start..start + len);
            start += len;
            sorted |= first.sort.is_some();
        }
        if sorted {
            features.sort_by(|a, b| decoded[a.start].1.sort.cmp(&decoded[b.start].1.sort));
        }

        for range in features.iter() {
            let pieces = &decoded[range.clone()];
            let first = &pieces[0].1;
            geom.clear();
            for (_, piece) in pieces {
                match piece.kind {
                    GeomKind::Fill | GeomKind::FillRange { .. } => push_square(geom, grid),
                    GeomKind::Point | GeomKind::Line | GeomKind::Polygon => {
                        piece.append_geometry(geom)?;
                    }
                }
            }
            let mut feature = layer.feature(geometry_type(first.kind, geom));
            feature.id(first.id);
            write_geometry(&mut feature, first.kind, geom)?;
            for prop in first.props() {
                let (key, value) = prop?;
                if let Some(pos) = columns.position(key)
                    && let Some(value) = value.to_value(schema[pos].1, text)
                {
                    feature.property(keys[pos], value)?;
                }
            }
            feature.finish()?;
        }
        Ok(AssembledLayer {
            layer,
            columns: schema,
        })
    }
}

fn push_square(geom: &mut GeomBuf, grid: LayerGrid) {
    #[expect(
        clippy::cast_possible_wrap,
        reason = "extent + buffer are far below i32::MAX"
    )]
    let (lo, hi) = (-(grid.buffer as i32), (grid.extent + grid.buffer) as i32);
    geom.polygons.push(1);
    geom.parts.push(4);
    geom.vertices
        .extend([[lo, lo], [hi, lo], [hi, hi], [lo, hi]]);
}

/// A single geometry unless the feature's pieces made several of them.
fn geometry_type(kind: GeomKind, geom: &GeomBuf) -> GeometryType {
    match kind {
        GeomKind::Point if geom.vertices.len() == 1 => GeometryType::Point,
        GeomKind::Point => GeometryType::MultiPoint,
        GeomKind::Line if geom.parts.len() == 1 => GeometryType::LineString,
        GeomKind::Line => GeometryType::MultiLineString,
        GeomKind::Polygon | GeomKind::Fill | GeomKind::FillRange { .. }
            if geom.polygons.len() == 1 =>
        {
            GeometryType::Polygon
        }
        GeomKind::Polygon | GeomKind::Fill | GeomKind::FillRange { .. } => {
            GeometryType::MultiPolygon
        }
    }
}

fn coords(part: &[Vertex]) -> impl Iterator<Item = Coord<i32>> + '_ {
    part.iter().map(|&[x, y]| Coord { x, y })
}

fn write_geometry(
    feature: &mut FeatureWriter<'_>,
    kind: GeomKind,
    geom: &GeomBuf,
) -> MltResult<()> {
    let mut vertices = geom.vertices.as_slice();
    let mut parts = geom.parts.iter().map(|&len| {
        let (part, rest) = vertices.split_at(len as usize);
        vertices = rest;
        part
    });
    match kind {
        GeomKind::Point => {
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
    use mlt_core::encoder::EncoderConfig;
    use mlt_core::geo_types::{Geometry, LineString, MultiLineString, Point};
    use mlt_core::{Decoder, Parser, PropValue, TileLayer};

    use super::*;
    use crate::props::{KeyInterner, PropRef};
    use crate::record::{EncodedProps, Geom, encode};

    const GRID: LayerGrid = LayerGrid {
        extent: 4096,
        buffer: 64,
    };

    fn record(
        id: Option<u64>,
        props: &[(&str, PropRef<'_>)],
        interner: &KeyInterner,
        geom: Geom<'_>,
    ) -> Vec<u8> {
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
                    &[("name", PropRef::Str("a")), ("height", PropRef::I64(3))],
                    &interner,
                    Geom::Points(&[[5, 6]]),
                ),
            ),
            (
                seq(2),
                record(
                    Some(11),
                    &[("name", PropRef::Str("b")), ("extra", PropRef::Bool(true))],
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
                    &[("name", PropRef::Str("b")), ("extra", PropRef::Bool(true))],
                    &interner,
                    Geom::Lines {
                        parts: &[2],
                        vertices: &[[20, 20], [30, 30]],
                    },
                ),
            ),
            (
                seq(3),
                record(None, &[("height", PropRef::I64(-1))], &interner, Geom::Fill),
            ),
        ];
        let names = interner.freeze();
        let records: Vec<_> = records.iter().map(|(s, b)| (*s, b.as_slice())).collect();
        let mut scratch = LayerAssembler::default();
        let assembled = scratch.assemble("roads", GRID, &names, &records).unwrap();
        let decoded = decode_mlt(&assembled.layer.encode(EncoderConfig::default()).unwrap());

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
        let records: Vec<_> = records.iter().map(|(s, b)| (*s, b.as_slice())).collect();
        let mut scratch = LayerAssembler::default();
        let assembled = scratch.assemble("roads", GRID, &names, &records).unwrap();
        let decoded = decode_mlt(&assembled.layer.encode(EncoderConfig::default()).unwrap());
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
            .assemble("water", GRID, &names, &[(Seq::default(), &bytes)])
            .unwrap();
        let decoded = decode_mlt(&assembled.layer.encode(EncoderConfig::default()).unwrap());
        let Geometry::MultiPolygon(multi) = decoded.features()[0].geometry() else {
            panic!("two polygons")
        };
        assert_eq!(multi.0.len(), 2);
        assert_eq!(multi.0[0].interiors().len(), 1);
        assert_eq!(multi.0[1].exterior().0.len(), 5, "geo_types closes rings");
    }
}

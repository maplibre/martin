use martin_core::tiles::Tile;
use martin_tile_utils::Format;

use crate::srv::tiles::content;
use crate::srv::tiles::process::ProcessError;

/// Convert an MLT tile to MVT (protobuf) format.
///
/// Handles decompression if the tile is compressed, then decodes MLT layers
/// into `TileLayer`s and re-encodes them as MVT protobuf.
pub fn convert_mlt_to_mvt(tile: Tile) -> Result<Tile, ProcessError> {
    use martin_tile_utils::{Encoding, TileInfo};

    let decoded =
        content::decode(tile).map_err(|e| ProcessError::DecompressionFailed(e.to_string()))?;

    let mvt_bytes =
        mlt_to_mvt_bytes(&decoded.data).map_err(|e| ProcessError::MvtConversion(e.clone()))?;

    Ok(Tile::new_hash_etag(
        mvt_bytes,
        TileInfo::new(Format::Mvt, Encoding::Uncompressed),
    ))
}

/// Decode MLT bytes into `TileLayer`s and encode them as MVT protobuf.
fn mlt_to_mvt_bytes(mlt_data: &[u8]) -> Result<Vec<u8>, String> {
    use mlt_core::{Decoder, Layer, Parser, TileLayer};
    use prost::Message as _;

    let mut parser = Parser::default();
    let layers = parser
        .parse_layers(mlt_data)
        .map_err(|e| format!("MLT parse failed: {e}"))?;

    let mut decoder = Decoder::default();
    let tile_layers: Vec<TileLayer> = layers
        .into_iter()
        .filter_map(|layer| match layer {
            Layer::Tag01(l) => Some(l.into_tile(&mut decoder)),
            _ => None,
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("MLT decode failed: {e}"))?;

    let mut proto_layers = Vec::with_capacity(tile_layers.len());
    for layer in tile_layers {
        proto_layers.push(tile_layer_to_mvt_layer(&layer)?);
    }

    let tile = mvt_proto::Tile {
        layers: proto_layers,
    };
    Ok(tile.encode_to_vec())
}

/// Convert a `TileLayer` into an MVT protobuf `Layer`.
fn tile_layer_to_mvt_layer(layer: &mlt_core::TileLayer) -> Result<mvt_proto::Layer, String> {
    use std::collections::HashMap;

    let mut keys: Vec<String> = Vec::new();
    let mut key_index: HashMap<String, u32> = HashMap::new();
    let mut values: Vec<mvt_proto::Value> = Vec::new();
    let mut value_index: HashMap<mvt_proto::Value, u32> = HashMap::new();

    let mut features = Vec::with_capacity(layer.features.len());

    for feat in &layer.features {
        let geom_type = geometry_type(&feat.geometry);
        let geometry = encode_geometry(&feat.geometry)?;

        let mut tags = Vec::new();
        for (col_idx, prop) in feat.properties.iter().enumerate() {
            if prop_is_null(prop) {
                continue;
            }
            let key_name = &layer.property_names[col_idx];
            let ki = *key_index.entry(key_name.clone()).or_insert_with(|| {
                let idx = u32::try_from(keys.len()).unwrap_or(u32::MAX);
                keys.push(key_name.clone());
                idx
            });

            let val = prop_to_mvt_value(prop);
            let vi = *value_index.entry(val.clone()).or_insert_with(|| {
                let idx = u32::try_from(values.len()).unwrap_or(u32::MAX);
                values.push(val);
                idx
            });

            tags.push(ki);
            tags.push(vi);
        }

        features.push(mvt_proto::Feature {
            id: feat.id,
            tags,
            r#type: Some(geom_type as i32),
            geometry,
        });
    }

    Ok(mvt_proto::Layer {
        version: 2,
        name: layer.name.clone(),
        features,
        keys,
        values,
        extent: Some(layer.extent),
    })
}

fn prop_is_null(prop: &mlt_core::PropValue) -> bool {
    use mlt_core::PropValue;
    matches!(
        prop,
        PropValue::Bool(None)
            | PropValue::I8(None)
            | PropValue::U8(None)
            | PropValue::I32(None)
            | PropValue::U32(None)
            | PropValue::I64(None)
            | PropValue::U64(None)
            | PropValue::F32(None)
            | PropValue::F64(None)
            | PropValue::Str(None)
    )
}

fn prop_to_mvt_value(prop: &mlt_core::PropValue) -> mvt_proto::Value {
    use mlt_core::PropValue;
    match prop {
        PropValue::Bool(Some(b)) => mvt_proto::Value {
            bool_value: Some(*b),
            ..Default::default()
        },
        PropValue::I8(Some(v)) => mvt_proto::Value {
            sint_value: Some(i64::from(*v)),
            ..Default::default()
        },
        PropValue::U8(Some(v)) => mvt_proto::Value {
            uint_value: Some(u64::from(*v)),
            ..Default::default()
        },
        PropValue::I32(Some(v)) => mvt_proto::Value {
            sint_value: Some(i64::from(*v)),
            ..Default::default()
        },
        PropValue::U32(Some(v)) => mvt_proto::Value {
            uint_value: Some(u64::from(*v)),
            ..Default::default()
        },
        PropValue::I64(Some(v)) => mvt_proto::Value {
            sint_value: Some(*v),
            ..Default::default()
        },
        PropValue::U64(Some(v)) => mvt_proto::Value {
            uint_value: Some(*v),
            ..Default::default()
        },
        PropValue::F32(Some(v)) => mvt_proto::Value {
            float_value: Some(*v),
            ..Default::default()
        },
        PropValue::F64(Some(v)) => mvt_proto::Value {
            double_value: Some(*v),
            ..Default::default()
        },
        PropValue::Str(Some(s)) => mvt_proto::Value {
            string_value: Some(s.clone()),
            ..Default::default()
        },
        _ => mvt_proto::Value::default(),
    }
}

fn geometry_type(geom: &mlt_core::geo_types::Geometry<i32>) -> mvt_proto::GeomType {
    use mlt_core::geo_types::Geometry;
    match geom {
        Geometry::Point(_) | Geometry::MultiPoint(_) => mvt_proto::GeomType::Point,
        Geometry::LineString(_) | Geometry::MultiLineString(_) => mvt_proto::GeomType::Linestring,
        Geometry::Polygon(_) | Geometry::MultiPolygon(_) => mvt_proto::GeomType::Polygon,
        _ => mvt_proto::GeomType::Unknown,
    }
}

/// Encode a geometry into MVT command/parameter integers.
///
/// MVT uses delta-encoded, zigzag-encoded coordinates with command integers:
/// - MoveTo(count): `(1 | count << 3)`
/// - LineTo(count): `(2 | count << 3)`
/// - `ClosePath`:     `(7 | 1 << 3)`
fn encode_geometry(geom: &mlt_core::geo_types::Geometry<i32>) -> Result<Vec<u32>, String> {
    use mlt_core::geo_types::Geometry;

    let mut out = Vec::new();
    let mut cx: i32 = 0;
    let mut cy: i32 = 0;

    match geom {
        Geometry::Point(p) => {
            encode_points(&[p.0], &mut out, &mut cx, &mut cy);
        }
        Geometry::MultiPoint(mp) => {
            let coords: Vec<_> = mp.iter().map(|p| p.0).collect();
            encode_points(&coords, &mut out, &mut cx, &mut cy);
        }
        Geometry::LineString(ls) => {
            encode_linestring(ls.0.as_slice(), &mut out, &mut cx, &mut cy);
        }
        Geometry::MultiLineString(mls) => {
            for ls in &mls.0 {
                encode_linestring(ls.0.as_slice(), &mut out, &mut cx, &mut cy);
            }
        }
        Geometry::Polygon(poly) => {
            encode_polygon(poly, &mut out, &mut cx, &mut cy);
        }
        Geometry::MultiPolygon(mp) => {
            for poly in &mp.0 {
                encode_polygon(poly, &mut out, &mut cx, &mut cy);
            }
        }
        _ => return Err(format!("Unsupported geometry type: {geom:?}")),
    }

    Ok(out)
}

fn encode_points(
    coords: &[mlt_core::geo_types::Coord<i32>],
    out: &mut Vec<u32>,
    cx: &mut i32,
    cy: &mut i32,
) {
    if coords.is_empty() {
        return;
    }
    // MoveTo(count)
    #[expect(clippy::cast_possible_truncation)]
    out.push(command_integer(1, coords.len() as u32));
    for c in coords {
        let dx = c.x - *cx;
        let dy = c.y - *cy;
        out.push(zigzag(dx));
        out.push(zigzag(dy));
        *cx = c.x;
        *cy = c.y;
    }
}

fn encode_linestring(
    coords: &[mlt_core::geo_types::Coord<i32>],
    out: &mut Vec<u32>,
    cx: &mut i32,
    cy: &mut i32,
) {
    if coords.is_empty() {
        return;
    }
    // MoveTo(1) for first point
    out.push(command_integer(1, 1));
    let first = &coords[0];
    out.push(zigzag(first.x - *cx));
    out.push(zigzag(first.y - *cy));
    *cx = first.x;
    *cy = first.y;

    // LineTo(count) for remaining points
    if coords.len() > 1 {
        #[expect(clippy::cast_possible_truncation)]
        out.push(command_integer(2, (coords.len() - 1) as u32));
        for c in &coords[1..] {
            out.push(zigzag(c.x - *cx));
            out.push(zigzag(c.y - *cy));
            *cx = c.x;
            *cy = c.y;
        }
    }
}

fn encode_polygon(
    poly: &mlt_core::geo_types::Polygon<i32>,
    out: &mut Vec<u32>,
    cx: &mut i32,
    cy: &mut i32,
) {
    encode_ring(poly.exterior().0.as_slice(), out, cx, cy);
    for interior in poly.interiors() {
        encode_ring(interior.0.as_slice(), out, cx, cy);
    }
}

fn encode_ring(
    coords: &[mlt_core::geo_types::Coord<i32>],
    out: &mut Vec<u32>,
    cx: &mut i32,
    cy: &mut i32,
) {
    // A ring must have at least 4 points (first == last in geo_types).
    // Drop the closing duplicate — MVT uses ClosePath instead.
    let ring = if coords.len() >= 2 && coords.first() == coords.last() {
        &coords[..coords.len() - 1]
    } else {
        coords
    };
    if ring.len() < 3 {
        return;
    }
    // MoveTo(1)
    out.push(command_integer(1, 1));
    out.push(zigzag(ring[0].x - *cx));
    out.push(zigzag(ring[0].y - *cy));
    *cx = ring[0].x;
    *cy = ring[0].y;

    // LineTo(count)
    #[expect(clippy::cast_possible_truncation)]
    out.push(command_integer(2, (ring.len() - 1) as u32));
    for c in &ring[1..] {
        out.push(zigzag(c.x - *cx));
        out.push(zigzag(c.y - *cy));
        *cx = c.x;
        *cy = c.y;
    }

    // ClosePath
    out.push(command_integer(7, 1));
}

/// Encode an MVT command integer: `(id & 0x7) | (count << 3)`
const fn command_integer(id: u32, count: u32) -> u32 {
    (id & 0x7) | (count << 3)
}

/// Zigzag-encode a signed 32-bit integer.
const fn zigzag(n: i32) -> u32 {
    ((n << 1) ^ (n >> 31)).cast_unsigned()
}

/// Minimal MVT protobuf types for encoding.
///
/// These mirror the Mapbox Vector Tile spec (v2.1) proto definitions
/// and derive `prost::Message` for zero-copy protobuf serialization.
mod mvt_proto {
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Tile {
        #[prost(message, repeated, tag = "3")]
        pub layers: Vec<Layer>,
    }

    #[derive(Clone, prost::Message)]
    pub struct Value {
        #[prost(string, optional, tag = "1")]
        pub string_value: Option<String>,
        #[prost(float, optional, tag = "2")]
        pub float_value: Option<f32>,
        #[prost(double, optional, tag = "3")]
        pub double_value: Option<f64>,
        #[prost(int64, optional, tag = "4")]
        pub int_value: Option<i64>,
        #[prost(uint64, optional, tag = "5")]
        pub uint_value: Option<u64>,
        #[prost(sint64, optional, tag = "6")]
        pub sint_value: Option<i64>,
        #[prost(bool, optional, tag = "7")]
        pub bool_value: Option<bool>,
    }

    impl PartialEq for Value {
        fn eq(&self, other: &Self) -> bool {
            self.string_value == other.string_value
                && self.float_value.map(f32::to_bits) == other.float_value.map(f32::to_bits)
                && self.double_value.map(f64::to_bits) == other.double_value.map(f64::to_bits)
                && self.int_value == other.int_value
                && self.uint_value == other.uint_value
                && self.sint_value == other.sint_value
                && self.bool_value == other.bool_value
        }
    }

    impl Eq for Value {}

    impl std::hash::Hash for Value {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.string_value.hash(state);
            self.float_value.map(f32::to_bits).hash(state);
            self.double_value.map(f64::to_bits).hash(state);
            self.int_value.hash(state);
            self.uint_value.hash(state);
            self.sint_value.hash(state);
            self.bool_value.hash(state);
        }
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Feature {
        #[prost(uint64, optional, tag = "1")]
        pub id: Option<u64>,
        #[prost(uint32, repeated, tag = "2")]
        pub tags: Vec<u32>,
        #[prost(enumeration = "GeomType", optional, tag = "3")]
        pub r#type: Option<i32>,
        #[prost(uint32, repeated, tag = "4")]
        pub geometry: Vec<u32>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Layer {
        #[prost(uint32, required, tag = "15")]
        pub version: u32,
        #[prost(string, required, tag = "1")]
        pub name: String,
        #[prost(message, repeated, tag = "2")]
        pub features: Vec<Feature>,
        #[prost(string, repeated, tag = "3")]
        pub keys: Vec<String>,
        #[prost(message, repeated, tag = "4")]
        pub values: Vec<Value>,
        #[prost(uint32, optional, tag = "5")]
        pub extent: Option<u32>,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, prost::Enumeration)]
    #[repr(i32)]
    pub enum GeomType {
        Unknown = 0,
        Point = 1,
        Linestring = 2,
        Polygon = 3,
    }
}

//! Encode `PostgreSQL` rows straight into MLT, without the MVT tile in between.

#[cfg(feature = "unstable-mlt-v2")]
mod nested;

use std::collections::HashMap;

use martin_tile_utils::{Encoding, Format, TileData, TileInfo};
use mlt_core::encoder::EncoderConfig;
#[cfg(feature = "unstable-mlt-v2")]
use mlt_core::encoder::WireVersion;
use mlt_core::geo_types::Geometry;
#[cfg(feature = "unstable-mlt-v2")]
use mlt_core::geo_types::{LineString, Polygon};
#[cfg(feature = "unstable-mlt-v2")]
use mlt_core::{MValue, MValueKey, TileLayerBuilder};
use mlt_core::{PropKind, PropValue, TileLayer};
use serde_json::{Number, Value};

use crate::tiles::Tile;
#[cfg(feature = "unstable-mlt-v2")]
use crate::tiles::postgres::PostgresFeature;
use crate::tiles::postgres::{
    PostgresError, PostgresProperty, PostgresResult, PostgresTileFeatures,
};

/// Encodes one tile's worth of `PostgreSQL` features as a single-layer MLT tile.
///
/// A tile without features encodes to an empty tile. M ordinates reach the layer's m-value
/// column and `jsonb` documents a nested column each, which only the v2 wire format has, so a
/// v1 tile leaves the M ordinates out and spreads each document's top-level keys over property
/// columns, as `ST_AsMVT` does.
pub fn encode_features_as_mlt(
    features: PostgresTileFeatures,
    cfg: EncoderConfig,
) -> PostgresResult<Tile> {
    let info = TileInfo::new(Format::Mlt, Encoding::Internal);
    let PostgresTileFeatures {
        layer_name,
        extent,
        features,
    } = features;
    if features.is_empty() {
        return Ok(Tile::new_hash_etag(TileData::new(), info));
    }

    let mut builder = TileLayer::builder(layer_name, extent).map_err(mlt_error)?;
    #[cfg(feature = "unstable-mlt-v2")]
    let measure_key = add_measure_column(&mut builder, &features, cfg)?;
    let mut columns = Columns::default();
    #[cfg(feature = "unstable-mlt-v2")]
    let mut documents = nested::Documents::default();
    let rows: Vec<_> = features
        .into_iter()
        .map(|feature| {
            let mut values = Vec::new();
            #[cfg(feature = "unstable-mlt-v2")]
            let mut docs = Vec::new();
            for (name, property) in feature.properties {
                match property {
                    PostgresProperty::Value(value) => columns.push(&name, value, &mut values),
                    #[cfg(feature = "unstable-mlt-v2")]
                    PostgresProperty::Json(document) if is_v2(cfg) => {
                        documents.push(&name, document, &mut docs);
                    }
                    PostgresProperty::Json(document) => {
                        for (key, value) in st_asmvt_properties(document) {
                            columns.push(&key, value, &mut values);
                        }
                    }
                }
            }
            #[cfg(feature = "unstable-mlt-v2")]
            let measures = stored_measures(&feature.geometry, feature.m_values);
            Row {
                id: feature.id,
                geometry: feature.geometry,
                #[cfg(feature = "unstable-mlt-v2")]
                measures,
                values,
                #[cfg(feature = "unstable-mlt-v2")]
                docs,
            }
        })
        .collect();

    let kinds = columns.kinds();
    let keys = columns
        .names
        .iter()
        .zip(&kinds)
        .map(|(name, kind)| builder.add_property(name.as_str(), *kind))
        .collect::<Result<Vec<_>, _>>()
        .map_err(mlt_error)?;
    #[cfg(feature = "unstable-mlt-v2")]
    let document_columns = documents.declare(&mut builder).map_err(mlt_error)?;

    for row in rows {
        let mut feature = builder.feature(row.geometry);
        feature.id(row.id);
        #[cfg(feature = "unstable-mlt-v2")]
        if let Some(key) = measure_key {
            feature
                .m_value(key, MValue::F64(row.measures))
                .map_err(mlt_error)?;
        }
        for (idx, value) in row.values {
            feature
                .property(keys[idx], to_prop_value(kinds[idx], value))
                .map_err(mlt_error)?;
        }
        #[cfg(feature = "unstable-mlt-v2")]
        for (idx, document) in row.docs {
            if let Some(column) = &document_columns[idx] {
                column.set(&mut feature, document).map_err(mlt_error)?;
            }
        }
        feature.finish().map_err(mlt_error)?;
    }

    let bytes = builder.finish().encode(cfg).map_err(mlt_error)?;
    Ok(Tile::new_hash_etag(bytes, info))
}

fn mlt_error(e: mlt_core::MltError) -> PostgresError {
    PostgresError::MltEncoding(Box::new(e))
}

/// One feature, its properties sorted into the layer's columns.
struct Row {
    id: Option<u64>,
    geometry: Geometry<i32>,
    #[cfg(feature = "unstable-mlt-v2")]
    measures: Option<Vec<f64>>,
    values: Vec<(usize, PropValue)>,
    #[cfg(feature = "unstable-mlt-v2")]
    docs: Vec<(usize, Value)>,
}

/// Whether tiles encoded with `cfg` use the v2 wire format.
#[cfg(feature = "unstable-mlt-v2")]
fn is_v2(cfg: EncoderConfig) -> bool {
    cfg.wire_version() != WireVersion::V01
}

/// Whether tiles encoded with `cfg` have an m-value column to keep M ordinates in, which only
/// the v2 wire format has.
#[cfg(feature = "unstable-mlt-v2")]
#[must_use]
pub fn keeps_measures(cfg: EncoderConfig) -> bool {
    is_v2(cfg)
}

/// Whether tiles encoded with `cfg` have an m-value column to keep M ordinates in, which only
/// the v2 wire format has.
#[cfg(not(feature = "unstable-mlt-v2"))]
#[must_use]
pub fn keeps_measures(_cfg: EncoderConfig) -> bool {
    false
}

/// The name the `PostGIS` M ordinate takes as the layer's vertex-scoped column.
#[cfg(feature = "unstable-mlt-v2")]
const MEASURE_COLUMN: &str = "m";

/// Declares the m-value column, unless nothing measured reaches a format that can hold it.
#[cfg(feature = "unstable-mlt-v2")]
fn add_measure_column(
    builder: &mut TileLayerBuilder,
    features: &[PostgresFeature],
    cfg: EncoderConfig,
) -> PostgresResult<Option<MValueKey>> {
    if !keeps_measures(cfg) || features.iter().all(|feature| feature.m_values.is_none()) {
        return Ok(None);
    }
    builder
        .add_m_value(MEASURE_COLUMN, PropKind::F64)
        .map(Some)
        .map_err(mlt_error)
}

/// One feature's M ordinates in the order MLT stores its vertices.
///
/// `parse_tile_wkb` keeps the `PostGIS` ring closing vertices that MLT omits, so the entries
/// standing for them go too.
#[cfg(feature = "unstable-mlt-v2")]
fn stored_measures(geometry: &Geometry<i32>, m_values: Option<Vec<f64>>) -> Option<Vec<f64>> {
    let m_values = m_values?;
    Some(match geometry {
        Geometry::Polygon(polygon) => strip_closing_measures(rings(polygon), &m_values),
        Geometry::MultiPolygon(polygons) => {
            strip_closing_measures(polygons.iter().flat_map(rings), &m_values)
        }
        Geometry::Point(_)
        | Geometry::Line(_)
        | Geometry::LineString(_)
        | Geometry::MultiPoint(_)
        | Geometry::MultiLineString(_)
        | Geometry::GeometryCollection(_)
        | Geometry::Rect(_)
        | Geometry::Triangle(_) => m_values,
    })
}

#[cfg(feature = "unstable-mlt-v2")]
fn rings(polygon: &Polygon<i32>) -> impl Iterator<Item = &LineString<i32>> {
    std::iter::once(polygon.exterior()).chain(polygon.interiors())
}

#[cfg(feature = "unstable-mlt-v2")]
fn strip_closing_measures<'a>(
    rings: impl Iterator<Item = &'a LineString<i32>>,
    m_values: &[f64],
) -> Vec<f64> {
    let mut stored = Vec::with_capacity(m_values.len());
    let mut at = 0;
    for ring in rings {
        let len = ring.0.len();
        let kept = len - usize::from(len > 1 && ring.0.last() == ring.0.first());
        stored.extend_from_slice(m_values.get(at..at + kept).unwrap_or_default());
        at += len;
    }
    stored
}

/// The layer's property columns, named and typed as the MVT round trip names and types them.
///
/// A column comes into being with its first non-`NULL` value, since `ST_AsMVT` writes no tag
/// for a `NULL`, so a column that is `NULL` for every feature is left out. Its type is one that
/// holds every value it was given, and an integer column is unsigned unless one of them is
/// negative.
#[derive(Default)]
struct Columns {
    names: Vec<String>,
    index: HashMap<String, usize>,
    kinds: Vec<PropKind>,
    signed: Vec<bool>,
}

impl Columns {
    /// Files one feature's `value` for the column `name` into `values`.
    fn push(&mut self, name: &str, value: PropValue, values: &mut Vec<(usize, PropValue)>) {
        if value.is_null() {
            return;
        }
        let kind = value.kind();
        let idx = if let Some(&idx) = self.index.get(name) {
            self.kinds[idx] = widen(self.kinds[idx], kind);
            idx
        } else {
            let idx = self.names.len();
            self.names.push(name.to_owned());
            self.index.insert(name.to_owned(), idx);
            self.kinds.push(kind);
            self.signed.push(false);
            idx
        };
        if matches!(value, PropValue::I64(Some(i)) if i < 0) {
            self.signed[idx] = true;
        }
        values.push((idx, value));
    }

    fn kinds(&self) -> Vec<PropKind> {
        self.kinds
            .iter()
            .zip(&self.signed)
            .map(|(&kind, &signed)| {
                if kind == PropKind::I64 && !signed {
                    PropKind::U64
                } else {
                    kind
                }
            })
            .collect()
    }
}

/// The type a column holding values of both types takes.
fn widen(a: PropKind, b: PropKind) -> PropKind {
    match (a, b) {
        _ if a == b => a,
        (PropKind::F32, PropKind::F64) | (PropKind::F64, PropKind::F32) => PropKind::F64,
        _ => PropKind::Str,
    }
}

/// One value in the column type [`Columns`] settled on.
fn to_prop_value(kind: PropKind, value: PropValue) -> PropValue {
    match (kind, value) {
        (PropKind::U64, PropValue::I64(v)) => PropValue::U64(v.and_then(|i| u64::try_from(i).ok())),
        (PropKind::F64, PropValue::F32(v)) => PropValue::F64(v.map(f64::from)),
        (PropKind::Str, PropValue::Bool(v)) => PropValue::Str(v.map(|v| v.to_string())),
        (PropKind::Str, PropValue::I64(v)) => PropValue::Str(v.map(|v| v.to_string())),
        (PropKind::Str, PropValue::F32(v)) => PropValue::Str(v.map(|v| v.to_string())),
        (PropKind::Str, PropValue::F64(v)) => PropValue::Str(v.map(|v| v.to_string())),
        (_, value) => value,
    }
}

/// The properties `ST_AsMVT` makes of a `jsonb` document: one for each top-level key holding a
/// string, a boolean or a number, and none for a document that is not an object.
fn st_asmvt_properties(document: Option<Value>) -> impl Iterator<Item = (String, PropValue)> {
    let object = match document {
        Some(Value::Object(object)) => object,
        _ => serde_json::Map::new(),
    };
    object.into_iter().filter_map(|(key, value)| {
        let value = match value {
            Value::String(s) => PropValue::Str(Some(s)),
            Value::Bool(b) => PropValue::Bool(Some(b)),
            Value::Number(n) => st_asmvt_number(&n),
            Value::Null | Value::Array(_) | Value::Object(_) => return None,
        };
        Some((key, value))
    })
}

/// A `jsonb` number as `ST_AsMVT` writes it: an integer when it lies within `f32::EPSILON` of
/// its integer part, a double otherwise.
fn st_asmvt_number(number: &Number) -> PropValue {
    if let Some(integer) = number.as_i64() {
        return PropValue::I64(Some(integer));
    }
    let Some(double) = number.as_f64() else {
        return PropValue::Str(Some(number.to_string()));
    };
    #[expect(
        clippy::cast_possible_truncation,
        reason = "saturates the way `strtol` does"
    )]
    let integer = double.trunc() as i64;
    #[expect(
        clippy::cast_precision_loss,
        reason = "compared the way `ST_AsMVT` does"
    )]
    let distance = (double - integer as f64).abs();
    if distance > f64::from(f32::EPSILON) {
        PropValue::F64(Some(double))
    } else {
        PropValue::I64(Some(integer))
    }
}

//! Encode `PostgreSQL` rows straight into MLT, without the MVT tile in between.

#[cfg(feature = "unstable-mlt-v2")]
mod nested;

use std::collections::HashMap;

use martin_core::tiles::Tile;
#[cfg(feature = "unstable-mlt-v2")]
use martin_core::tiles::postgres::PostgresFeature;
use martin_core::tiles::postgres::{PostgresProperty, PostgresTileFeatures};
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

use crate::srv::tiles::process::ProcessError;

/// Encodes one tile's worth of `PostgreSQL` features as a single-layer MLT tile.
///
/// A tile without features encodes to an empty tile. M ordinates reach the layer's m-value
/// column and `jsonb` documents a nested column each, which only the v2 wire format has, so a
/// v1 tile leaves the M ordinates out and spreads each document's top-level keys over property
/// columns, as `ST_AsMVT` does.
pub(crate) fn encode_features_as_mlt(
    features: PostgresTileFeatures,
    cfg: EncoderConfig,
) -> Result<Tile, ProcessError> {
    let info = TileInfo::new(Format::Mlt, Encoding::Internal);
    let PostgresTileFeatures {
        layer_name,
        extent,
        features,
    } = features;
    if features.is_empty() {
        return Ok(Tile::new_hash_etag(TileData::new(), info));
    }

    let mut builder = TileLayer::builder(layer_name, extent).map_err(|e| mlt_error(&e))?;
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
        .map_err(|e| mlt_error(&e))?;
    #[cfg(feature = "unstable-mlt-v2")]
    let document_columns = documents.declare(&mut builder).map_err(|e| mlt_error(&e))?;

    for row in rows {
        let mut feature = builder.feature(row.geometry);
        feature.id(row.id);
        #[cfg(feature = "unstable-mlt-v2")]
        if let Some(key) = measure_key {
            feature
                .m_value(key, MValue::F64(row.measures))
                .map_err(|e| mlt_error(&e))?;
        }
        for (idx, value) in row.values {
            feature
                .property(keys[idx], to_prop_value(kinds[idx], value))
                .map_err(|e| mlt_error(&e))?;
        }
        #[cfg(feature = "unstable-mlt-v2")]
        for (idx, document) in row.docs {
            if let Some(column) = &document_columns[idx] {
                column
                    .set(&mut feature, document)
                    .map_err(|e| mlt_error(&e))?;
            }
        }
        feature.finish().map_err(|e| mlt_error(&e))?;
    }

    let bytes = builder.finish().encode(cfg).map_err(|e| mlt_error(&e))?;
    Ok(Tile::new_hash_etag(bytes, info))
}

fn mlt_error(e: &mlt_core::MltError) -> ProcessError {
    ProcessError::MltEncoding(e.to_string())
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
pub(crate) fn keeps_measures(cfg: EncoderConfig) -> bool {
    is_v2(cfg)
}

/// Whether tiles encoded with `cfg` have an m-value column to keep M ordinates in, which only
/// the v2 wire format has.
#[cfg(not(feature = "unstable-mlt-v2"))]
pub(crate) fn keeps_measures(_cfg: EncoderConfig) -> bool {
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
) -> Result<Option<MValueKey>, ProcessError> {
    if !keeps_measures(cfg) || features.iter().all(|feature| feature.m_values.is_none()) {
        return Ok(None);
    }
    builder
        .add_m_value(MEASURE_COLUMN, PropKind::F64)
        .map(Some)
        .map_err(|e| mlt_error(&e))
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

#[cfg(test)]
mod tests {
    use martin_core::tiles::postgres::PostgresFeature;
    use mlt_core::geo_types::{Coord, Point};
    use mlt_core::{Decoder, Parser};
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    fn point(x: i32, y: i32) -> Geometry<i32> {
        Geometry::Point(Point(Coord { x, y }))
    }

    fn feature(properties: Vec<(&str, PostgresProperty)>) -> PostgresFeature {
        PostgresFeature {
            id: None,
            geometry: point(1, 2),
            m_values: None,
            properties: properties
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        }
    }

    fn tile(features: Vec<PostgresFeature>) -> PostgresTileFeatures {
        PostgresTileFeatures {
            layer_name: "layer".to_owned(),
            extent: 4096,
            features,
        }
    }

    fn decode(data: &[u8]) -> Vec<TileLayer> {
        let mut parser = Parser::default();
        let mut decoder = Decoder::default();
        parser
            .parse_layers(data)
            .expect("the encoded tile does not parse")
            .into_iter()
            .map(|layer| {
                layer
                    .into_layer01()
                    .expect("unknown layer tag")
                    .into_tile(&mut decoder)
                    .expect("undecodable layer")
            })
            .collect()
    }

    fn encode(features: Vec<PostgresFeature>, cfg: EncoderConfig) -> TileLayer {
        let encoded = encode_features_as_mlt(tile(features), cfg).expect("encoding the tile");
        let [layer] = decode(&encoded.data).try_into().expect("one layer");
        layer
    }

    /// Every feature's properties by column name, leaving out the `NULL`s.
    fn properties(layer: &TileLayer) -> Vec<Vec<(String, PropValue)>> {
        layer
            .features()
            .iter()
            .map(|feature| {
                layer
                    .property_names()
                    .iter()
                    .cloned()
                    .zip(feature.properties().iter().cloned())
                    .filter(|(_, value)| !value.is_null())
                    .collect()
            })
            .collect()
    }

    fn value(value: PropValue) -> PostgresProperty {
        PostgresProperty::Value(value)
    }

    fn doc(document: Value) -> PostgresProperty {
        PostgresProperty::Json(Some(document))
    }

    fn str(s: &str) -> PropValue {
        PropValue::Str(Some(s.to_owned()))
    }

    #[rstest]
    #[case::boolean(PropValue::Bool(Some(true)), PropKind::Bool)]
    #[case::float(PropValue::F32(Some(1.5)), PropKind::F32)]
    #[case::double(PropValue::F64(Some(1.5)), PropKind::F64)]
    #[case::text(str("x"), PropKind::Str)]
    #[case::non_negative_integer(PropValue::I64(Some(7)), PropKind::U64)]
    #[case::negative_integer(PropValue::I64(Some(-7)), PropKind::I64)]
    fn a_column_takes_the_type_of_the_values_in_it(
        #[case] v: PropValue,
        #[case] expected: PropKind,
    ) {
        let mut columns = Columns::default();
        columns.push("p", v, &mut Vec::new());
        assert_eq!(columns.kinds(), [expected]);
    }

    #[test]
    fn one_negative_value_makes_the_whole_integer_column_signed() {
        let mut columns = Columns::default();
        for v in [Some(7), None, Some(-1)] {
            columns.push("p", PropValue::I64(v), &mut Vec::new());
        }
        assert_eq!(columns.kinds(), [PropKind::I64]);
    }

    #[test]
    fn a_column_that_is_null_everywhere_is_left_out() {
        let features = vec![
            feature(vec![
                ("empty", value(PropValue::I64(None))),
                ("filled", value(str("a"))),
            ]),
            feature(vec![
                ("empty", value(PropValue::I64(None))),
                ("filled", value(PropValue::Str(None))),
            ]),
        ];
        let layer = encode(features, EncoderConfig::default());
        assert_eq!(layer.property_names(), ["filled"]);
        assert_eq!(
            layer.features()[1].properties(),
            [PropValue::Str(None)],
            "a NULL must stay a NULL rather than become a default"
        );
    }

    #[test]
    fn columns_come_in_the_order_their_first_value_does() {
        let features = vec![
            feature(vec![
                ("a", value(PropValue::I64(None))),
                ("b", value(str("x"))),
            ]),
            feature(vec![("a", value(str("y"))), ("b", value(str("z")))]),
        ];
        let layer = encode(features, EncoderConfig::default());
        assert_eq!(layer.property_names(), ["b", "a"]);
    }

    #[test]
    fn an_empty_result_set_encodes_to_an_empty_tile() {
        let encoded = encode_features_as_mlt(tile(Vec::new()), EncoderConfig::default())
            .expect("encoding a tile without features");
        assert!(encoded.data.is_empty());
        assert_eq!(encoded.info, TileInfo::new(Format::Mlt, Encoding::Internal));
    }

    #[test]
    fn a_feature_id_and_its_properties_survive_the_encoding() {
        let features = vec![PostgresFeature {
            id: Some(42),
            geometry: point(10, 20),
            m_values: Some(vec![1.0]),
            properties: vec![
                ("n".into(), value(PropValue::I64(Some(-5)))),
                ("s".into(), value(str("hi"))),
            ],
        }];
        let layer = encode(features, EncoderConfig::default());
        assert_eq!(layer.name(), "layer");
        assert_eq!(layer.extent().get(), 4096);
        assert_eq!(layer.property_names(), ["n", "s"]);
        let [feature] = layer.features() else {
            panic!("one feature");
        };
        assert_eq!(feature.id(), Some(42));
        assert_eq!(feature.geometry(), &point(10, 20));
        assert_eq!(feature.properties(), [PropValue::I32(Some(-5)), str("hi")]);
    }

    #[test]
    fn a_v1_document_spreads_its_top_level_scalars_over_columns() {
        let document = json!({
            "s": "x",
            "b": true,
            "i": 3,
            "n": null,
            "o": {"k": 1},
            "arr": [1, 2],
        });
        let layer = encode(
            vec![feature(vec![("doc", doc(document))])],
            EncoderConfig::default(),
        );
        assert_eq!(
            properties(&layer),
            [vec![
                ("s".to_owned(), str("x")),
                ("b".to_owned(), PropValue::Bool(Some(true))),
                ("i".to_owned(), PropValue::U32(Some(3))),
            ]]
        );
    }

    #[rstest]
    #[case::integer(json!(3), PropValue::I64(Some(3)))]
    #[case::negative(json!(-4), PropValue::I64(Some(-4)))]
    #[case::whole_double(json!(2.0), PropValue::I64(Some(2)))]
    #[case::within_float_epsilon(json!(1.000_000_01), PropValue::I64(Some(1)))]
    #[case::fraction(json!(1.5), PropValue::F64(Some(1.5)))]
    #[case::past_i64(json!(12_345_678_901_234_567_890_u64), PropValue::F64(Some(1.234_567_890_123_456_8e19)))]
    #[case::huge(json!(1e20), PropValue::F64(Some(1e20)))]
    fn a_v1_document_number_is_typed_as_st_asmvt_types_it(
        #[case] number: Value,
        #[case] expected: PropValue,
    ) {
        let Value::Number(number) = number else {
            panic!("not a number");
        };
        assert_eq!(st_asmvt_number(&number), expected);
    }

    #[rstest]
    #[case::array(json!([{"x": 1}]))]
    #[case::scalar(json!("scalar"))]
    #[case::null(Value::Null)]
    fn a_v1_document_that_is_no_object_has_no_properties(#[case] document: Value) {
        assert_eq!(st_asmvt_properties(Some(document)).count(), 0);
    }

    #[test]
    fn a_v1_document_key_shares_the_column_of_the_same_name() {
        let features = vec![
            feature(vec![
                ("a", value(PropValue::I64(Some(5)))),
                ("doc", doc(json!({"a": "dup"}))),
            ]),
            feature(vec![
                ("a", value(PropValue::I64(Some(6)))),
                ("doc", doc(json!({}))),
            ]),
        ];
        let layer = encode(features, EncoderConfig::default());
        assert_eq!(
            properties(&layer),
            [
                vec![("a".to_owned(), str("dup"))],
                vec![("a".to_owned(), str("6"))],
            ],
            "the later value wins, and a column holding text and integers holds text"
        );
    }

    #[test]
    fn a_v1_key_holding_integers_and_doubles_holds_text_like_the_mvt_round_trip() {
        let features = vec![
            feature(vec![("doc", doc(json!({"f": 1.5})))]),
            feature(vec![("doc", doc(json!({"f": 2})))]),
        ];
        let layer = encode(features, EncoderConfig::default());
        assert_eq!(
            properties(&layer),
            [
                vec![("f".to_owned(), str("1.5"))],
                vec![("f".to_owned(), str("2"))],
            ]
        );
    }

    #[cfg(feature = "unstable-mlt-v2")]
    mod documents {
        use mlt_core::encoder::WireVersion;
        use mlt_core::{NestedKind, NestedValue};

        use super::*;

        fn v2() -> EncoderConfig {
            EncoderConfig::default().with_wire_version(WireVersion::V02)
        }

        fn leaf(value: PropValue) -> NestedValue {
            NestedValue::Leaf(value)
        }

        #[test]
        fn a_v2_document_is_kept_whole_in_a_nested_column() {
            let document = json!({
                "s": "x",
                "n": null,
                "o": {"k": 1, "deeper": {"flag": true}},
                "arr": [1.5, 2],
            });
            let layer = encode(vec![feature(vec![("doc", doc(document))])], v2());
            assert!(layer.property_names().is_empty());
            assert_eq!(layer.nested_names(), ["doc"]);
            assert_eq!(
                layer.nested_kinds(),
                [NestedKind::map([
                    ("arr", NestedKind::list(NestedKind::Leaf(PropKind::F64))),
                    (
                        "o",
                        NestedKind::map([
                            (
                                "deeper",
                                NestedKind::map([("flag", NestedKind::Leaf(PropKind::Bool))])
                            ),
                            ("k", NestedKind::Leaf(PropKind::I64)),
                        ])
                    ),
                    ("s", NestedKind::Leaf(PropKind::Str)),
                ])]
            );
            assert_eq!(
                layer.features()[0].nested(),
                [NestedValue::map([
                    (
                        "arr",
                        NestedValue::list([
                            leaf(PropValue::F64(Some(1.5))),
                            leaf(PropValue::F64(Some(2.0)))
                        ])
                    ),
                    (
                        "o",
                        NestedValue::map([
                            (
                                "deeper",
                                NestedValue::map([("flag", leaf(PropValue::Bool(Some(true))))])
                            ),
                            ("k", leaf(PropValue::I64(Some(1)))),
                        ])
                    ),
                    ("s", leaf(str("x"))),
                ])]
            );
        }

        #[test]
        fn values_that_share_no_shape_are_kept_as_json_text() {
            let features = vec![
                feature(vec![("doc", doc(json!({"v": {"k": 1}})))]),
                feature(vec![("doc", doc(json!({"v": 7})))]),
            ];
            let layer = encode(features, v2());
            assert_eq!(
                layer.nested_kinds(),
                [NestedKind::map([("v", NestedKind::Leaf(PropKind::Str))])]
            );
            assert_eq!(
                layer
                    .features()
                    .iter()
                    .map(|f| f.nested()[0].clone())
                    .collect::<Vec<_>>(),
                [
                    NestedValue::map([("v", leaf(str(r#"{"k":1}"#)))]),
                    NestedValue::map([("v", leaf(str("7")))]),
                ]
            );
        }

        #[test]
        fn a_column_of_empty_or_null_documents_is_left_out() {
            let features = vec![
                feature(vec![("empty", doc(json!({"a": {}, "b": null})))]),
                feature(vec![("empty", PostgresProperty::Json(None))]),
            ];
            let layer = encode(features, v2());
            assert!(layer.nested_names().is_empty());
            assert!(layer.property_names().is_empty());
        }

        #[test]
        fn a_column_of_scalar_documents_is_an_ordinary_property() {
            let features = vec![
                feature(vec![("doc", doc(json!(3)))]),
                feature(vec![("doc", doc(json!("x")))]),
            ];
            let layer = encode(features, v2());
            assert!(layer.nested_names().is_empty());
            assert_eq!(
                properties(&layer),
                [
                    vec![("doc".to_owned(), str("3"))],
                    vec![("doc".to_owned(), str("x"))],
                ]
            );
        }

        #[test]
        fn a_level_too_deep_for_the_wire_is_kept_as_json_text() {
            let document = json!({"1": {"2": {"3": {"4": {"5": {"6": {"7": {"8": {"9": 1}}}}}}}}});
            let layer = encode(vec![feature(vec![("doc", doc(document))])], v2());
            let mut kind = &layer.nested_kinds()[0];
            let mut depth = 1;
            while let NestedKind::Map(fields) = kind {
                kind = fields.values().next().expect("a field");
                depth += 1;
            }
            assert_eq!(depth, 8);
            assert_eq!(kind, &NestedKind::Leaf(PropKind::Str));
        }
    }

    #[cfg(feature = "unstable-mlt-v2")]
    mod m_values {
        use mlt_core::geo_types::{LineString, MultiPolygon, Polygon};

        use super::*;

        fn v2() -> EncoderConfig {
            EncoderConfig::default().with_wire_version(WireVersion::V02)
        }

        fn coords(coords: &[(i32, i32)]) -> LineString<i32> {
            LineString(coords.iter().map(|&(x, y)| Coord { x, y }).collect())
        }

        fn square(offset: i32) -> Polygon<i32> {
            Polygon::new(
                coords(&[
                    (offset, 0),
                    (offset + 4, 0),
                    (offset + 4, 4),
                    (offset, 4),
                    (offset, 0),
                ]),
                Vec::new(),
            )
        }

        fn measured(geometry: Geometry<i32>, m_values: Option<Vec<f64>>) -> PostgresFeature {
            PostgresFeature {
                id: None,
                geometry,
                m_values,
                properties: Vec::new(),
            }
        }

        fn encoded_m_values(features: Vec<PostgresFeature>, cfg: EncoderConfig) -> Vec<MValue> {
            let encoded =
                encode_features_as_mlt(tile(features), cfg).expect("encoding a measured tile");
            let [layer] = decode(&encoded.data).try_into().expect("one layer");
            layer
                .features()
                .iter()
                .map(|feature| {
                    feature
                        .m_values()
                        .first()
                        .cloned()
                        .unwrap_or(MValue::F64(None))
                })
                .collect()
        }

        #[test]
        fn a_measured_line_keeps_one_value_per_vertex() {
            let line = Geometry::LineString(coords(&[(0, 0), (10, 10), (20, 5)]));
            let features = vec![measured(line, Some(vec![1.5, 2.5, 3.5]))];
            let encoded = encode_features_as_mlt(tile(features), v2())
                .expect("encoding a measured linestring");
            let [layer] = decode(&encoded.data).try_into().expect("one layer");
            assert_eq!(layer.m_value_names(), ["m"]);
            assert_eq!(layer.m_value_kinds(), [PropKind::F64]);
            assert_eq!(
                layer.features()[0].m_values(),
                [MValue::F64(Some(vec![1.5, 2.5, 3.5]))]
            );
        }

        #[test]
        fn a_ring_loses_the_measure_of_its_closing_vertex() {
            let features = vec![measured(
                Geometry::Polygon(square(0)),
                Some(vec![1.0, 2.0, 3.0, 4.0, 1.0]),
            )];
            assert_eq!(
                encoded_m_values(features, v2()),
                [MValue::F64(Some(vec![1.0, 2.0, 3.0, 4.0]))]
            );
        }

        #[test]
        fn every_ring_of_a_multipolygon_loses_its_closing_vertex() {
            let geometry = Geometry::MultiPolygon(MultiPolygon(vec![square(0), square(10)]));
            let m_values = Some(vec![1.0, 2.0, 3.0, 4.0, 1.0, 5.0, 6.0, 7.0, 8.0, 5.0]);
            assert_eq!(
                encoded_m_values(vec![measured(geometry, m_values)], v2()),
                [MValue::F64(Some(vec![
                    1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0
                ]))]
            );
        }

        #[test]
        fn an_interior_ring_loses_its_closing_vertex_too() {
            let polygon = Polygon::new(
                coords(&[(0, 0), (100, 0), (100, 100), (0, 100), (0, 0)]),
                vec![coords(&[(10, 10), (20, 10), (20, 20), (10, 20), (10, 10)])],
            );
            let m_values = Some(vec![1.0, 2.0, 3.0, 4.0, 1.0, 5.0, 6.0, 7.0, 8.0, 5.0]);
            assert_eq!(
                stored_measures(&Geometry::Polygon(polygon), m_values),
                Some(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0])
            );
        }

        #[test]
        fn an_unmeasured_feature_nulls_the_whole_column() {
            let measured_line = measured(
                Geometry::LineString(coords(&[(0, 0), (1, 1)])),
                Some(vec![7.0, 8.0]),
            );
            let plain_line = measured(Geometry::LineString(coords(&[(2, 2), (3, 3)])), None);
            assert_eq!(
                encoded_m_values(vec![measured_line, plain_line], v2()),
                [MValue::F64(Some(vec![7.0, 8.0])), MValue::F64(None)]
            );
        }

        #[test]
        fn v1_has_nowhere_to_put_them_so_it_drops_them() {
            let line = Geometry::LineString(coords(&[(0, 0), (10, 10)]));
            let features = vec![measured(line, Some(vec![1.0, 2.0]))];
            let encoded = encode_features_as_mlt(tile(features), EncoderConfig::default())
                .expect("a measured tile still encodes as v1");
            let [layer] = decode(&encoded.data).try_into().expect("one layer");
            assert!(layer.m_value_names().is_empty());
        }

        #[test]
        fn a_property_column_named_m_collides_with_them() {
            let mut feature = measured(
                Geometry::LineString(coords(&[(0, 0), (1, 1)])),
                Some(vec![1.0, 2.0]),
            );
            feature.properties = vec![("m".into(), value(PropValue::I64(Some(1))))];
            encode_features_as_mlt(tile(vec![feature]), v2())
                .expect_err("the m-value column cannot share a name with a property");
        }

        #[test]
        fn a_point_layer_cannot_carry_them() {
            let features = vec![measured(point(1, 2), Some(vec![1.0]))];
            let err = encode_features_as_mlt(tile(features), v2())
                .expect_err("a point layout gives no per-feature vertex count");
            let ProcessError::MltEncoding(message) = err else {
                panic!("the encoding should fail on the m-value column");
            };
            assert!(message.contains("m-values"), "unexpected error: {message}");
        }
    }
}

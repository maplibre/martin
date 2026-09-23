//! Encode `PostgreSQL` rows straight into MLT, without the MVT tile in between.

use martin_core::tiles::Tile;
use martin_core::tiles::postgres::{PostgresFeature, PostgresTileFeatures};
use martin_tile_utils::{Encoding, Format, TileData, TileInfo};
use mlt_core::encoder::EncoderConfig;
#[cfg(feature = "unstable-mlt-v2")]
use mlt_core::encoder::WireVersion;
#[cfg(feature = "unstable-mlt-v2")]
use mlt_core::geo_types::{Geometry, LineString, Polygon};
#[cfg(feature = "unstable-mlt-v2")]
use mlt_core::{MValue, MValueKey, TileLayerBuilder};
use mlt_core::{PropKind, PropValue, TileLayer};

use crate::srv::tiles::process::ProcessError;

/// Encodes one tile's worth of `PostgreSQL` features as a single-layer MLT tile.
///
/// A tile without features encodes to an empty tile. M ordinates reach the layer's m-value
/// column, which only the v2 wire format has, so a v1 tile leaves them out.
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
    let Some(first) = features.first() else {
        return Ok(Tile::new_hash_etag(TileData::new(), info));
    };

    let kinds = column_kinds(&features);
    let names: Vec<_> = first
        .properties
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    let mut builder = TileLayer::builder(layer_name, extent)
        .map_err(|e| ProcessError::MltEncoding(e.to_string()))?;
    let mut keys = Vec::with_capacity(kinds.len());
    for (name, kind) in names.iter().zip(&kinds) {
        keys.push(match kind {
            Some(kind) => Some(
                builder
                    .add_property(name.as_str(), *kind)
                    .map_err(|e| ProcessError::MltEncoding(e.to_string()))?,
            ),
            None => None,
        });
    }
    #[cfg(feature = "unstable-mlt-v2")]
    let measure_key = add_measure_column(&mut builder, &features, cfg)?;

    for feature in features {
        #[cfg(feature = "unstable-mlt-v2")]
        let measures = stored_measures(&feature.geometry, feature.m_values);
        let mut row = builder.feature(feature.geometry);
        row.id(feature.id);
        #[cfg(feature = "unstable-mlt-v2")]
        if let Some(key) = measure_key {
            row.m_value(key, MValue::F64(measures))
                .map_err(|e| ProcessError::MltEncoding(e.to_string()))?;
        }
        for ((_, value), (key, kind)) in feature.properties.into_iter().zip(keys.iter().zip(&kinds))
        {
            let (Some(key), Some(kind)) = (key, kind) else {
                continue;
            };
            row.property(*key, to_prop_value(*kind, value))
                .map_err(|e| ProcessError::MltEncoding(e.to_string()))?;
        }
        row.finish()
            .map_err(|e| ProcessError::MltEncoding(e.to_string()))?;
    }

    let bytes = builder
        .finish()
        .encode(cfg)
        .map_err(|e| ProcessError::MltEncoding(e.to_string()))?;
    Ok(Tile::new_hash_etag(bytes, info))
}

/// Whether tiles encoded with `cfg` have an m-value column to keep M ordinates in, which only
/// the v2 wire format has.
#[cfg(feature = "unstable-mlt-v2")]
pub(crate) fn keeps_measures(cfg: EncoderConfig) -> bool {
    cfg.wire_version() != WireVersion::V01
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
        .map_err(|e| ProcessError::MltEncoding(e.to_string()))
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

/// The MLT type of every property column, or `None` for a column to leave out of the layer.
///
/// Matches what the MVT round-trip arrives at: an all-`NULL` column is dropped, and an integer
/// column is unsigned unless the tile holds a negative value for it.
fn column_kinds(features: &[PostgresFeature]) -> Vec<Option<PropKind>> {
    let Some(first) = features.first() else {
        return Vec::new();
    };
    (0..first.properties.len())
        .map(|idx| {
            let mut values = features
                .iter()
                .filter_map(|f| f.properties.get(idx).map(|(_, value)| value))
                .filter(|value| !value.is_null())
                .peekable();
            let kind = values.peek()?.kind();
            if kind == PropKind::I64
                && !values.any(|v| matches!(v, PropValue::I64(Some(i)) if *i < 0))
            {
                return Some(PropKind::U64);
            }
            Some(kind)
        })
        .collect()
}

/// One value in the column type [`column_kinds`] settled on, `NULL` included.
fn to_prop_value(kind: PropKind, value: PropValue) -> PropValue {
    if kind == PropKind::U64
        && let PropValue::I64(v) = value
    {
        return PropValue::U64(v.and_then(|i| u64::try_from(i).ok()));
    }
    value
}

#[cfg(test)]
mod tests {
    use mlt_core::geo_types::{Coord, Geometry, Point};
    use mlt_core::{Decoder, Layer, Parser};
    use rstest::rstest;

    use super::*;

    fn point(x: i32, y: i32) -> Geometry<i32> {
        Geometry::Point(Point(Coord { x, y }))
    }

    fn feature(properties: Vec<(&str, PropValue)>) -> PostgresFeature {
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
                let layer = match layer {
                    Layer::Tag01(layer) => layer,
                    #[cfg(feature = "unstable-mlt-v2")]
                    Layer::Tag02(layer) => layer,
                    Layer::Unknown(unknown) => panic!("unknown layer tag {}", unknown.tag()),
                    _ => panic!("an unhandled layer variant"),
                };
                layer.into_tile(&mut decoder).expect("undecodable layer")
            })
            .collect()
    }

    #[rstest]
    #[case::boolean(PropValue::Bool(Some(true)), PropKind::Bool)]
    #[case::float(PropValue::F32(Some(1.5)), PropKind::F32)]
    #[case::double(PropValue::F64(Some(1.5)), PropKind::F64)]
    #[case::text(PropValue::Str(Some("x".to_owned())), PropKind::Str)]
    #[case::non_negative_integer(PropValue::I64(Some(7)), PropKind::U64)]
    #[case::negative_integer(PropValue::I64(Some(-7)), PropKind::I64)]
    fn a_column_takes_the_type_of_the_values_in_it(
        #[case] value: PropValue,
        #[case] expected: PropKind,
    ) {
        let features = vec![feature(vec![("p", value)])];
        assert_eq!(column_kinds(&features), vec![Some(expected)]);
    }

    #[test]
    fn one_negative_value_makes_the_whole_integer_column_signed() {
        let features = vec![
            feature(vec![("p", PropValue::I64(Some(7)))]),
            feature(vec![("p", PropValue::I64(None))]),
            feature(vec![("p", PropValue::I64(Some(-1)))]),
        ];
        assert_eq!(column_kinds(&features), vec![Some(PropKind::I64)]);
    }

    #[test]
    fn a_column_that_is_null_everywhere_is_left_out() {
        let features = vec![
            feature(vec![
                ("empty", PropValue::I64(None)),
                ("filled", PropValue::Str(Some("a".to_owned()))),
            ]),
            feature(vec![
                ("empty", PropValue::I64(None)),
                ("filled", PropValue::Str(None)),
            ]),
        ];
        assert_eq!(column_kinds(&features), vec![None, Some(PropKind::Str)]);

        let encoded = encode_features_as_mlt(tile(features), EncoderConfig::default())
            .expect("encoding a two-column tile");
        let [layer] = decode(&encoded.data).try_into().expect("one layer");
        assert_eq!(layer.property_names(), ["filled"]);
        assert_eq!(
            layer.features()[1].properties(),
            [PropValue::Str(None)],
            "a NULL must stay a NULL rather than become a default"
        );
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
                ("n".into(), PropValue::I64(Some(-5))),
                ("s".into(), PropValue::Str(Some("hi".to_owned()))),
            ],
        }];
        let encoded = encode_features_as_mlt(tile(features), EncoderConfig::default())
            .expect("encoding one feature");
        let [layer] = decode(&encoded.data).try_into().expect("one layer");
        assert_eq!(layer.name(), "layer");
        assert_eq!(layer.extent().get(), 4096);
        assert_eq!(layer.property_names(), ["n", "s"]);
        let [feature] = layer.features() else {
            panic!("one feature");
        };
        assert_eq!(feature.id(), Some(42));
        assert_eq!(feature.geometry(), &point(10, 20));
        assert_eq!(
            feature.properties(),
            [
                PropValue::I32(Some(-5)),
                PropValue::Str(Some("hi".to_owned()))
            ]
        );
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
            feature.properties = vec![("m".into(), PropValue::I64(Some(1)))];
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

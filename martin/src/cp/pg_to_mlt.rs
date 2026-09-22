//! Encode `PostgreSQL` rows straight into MLT, without the MVT tile in between.

use martin_core::tiles::Tile;
use martin_core::tiles::postgres::{PostgresFeature, PostgresTileFeatures};
use martin_tile_utils::{Encoding, Format, TileData, TileInfo};
use mlt_core::encoder::EncoderConfig;
use mlt_core::{PropKind, PropValue, TileLayer};

use crate::srv::tiles::process::ProcessError;

/// Encodes one tile's worth of `PostgreSQL` features as a single-layer MLT tile.
///
/// A tile without features encodes to an empty tile.
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

    for feature in features {
        let mut row = builder.feature(feature.geometry);
        row.id(feature.id);
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
                let Layer::Tag01(layer) = layer else {
                    panic!("the encoded layer is not MVT-compatible");
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
}

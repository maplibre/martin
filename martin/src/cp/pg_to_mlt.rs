//! Encode `PostgreSQL` rows straight into MLT, without the MVT tile in between.

use martin_core::tiles::Tile;
use martin_core::tiles::postgres::{
    PostgresFeature, PostgresPropValue, PostgresTileFeatures, TileGeometry, TileVertex,
};
use martin_tile_utils::{Encoding, Format, TileData, TileInfo};
use mlt_core::encoder::EncoderConfig;
use mlt_core::geo_types::{
    Coord, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};
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
        let mut row = builder.feature(to_encodable_geometry(feature.geometry));
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
            let first = *values.peek()?;
            Some(match first {
                PostgresPropValue::Bool(_) => PropKind::Bool,
                PostgresPropValue::Float(_) => PropKind::F32,
                PostgresPropValue::Double(_) => PropKind::F64,
                PostgresPropValue::Text(_) => PropKind::Str,
                PostgresPropValue::Int(_) => {
                    if values.any(|v| matches!(v, PostgresPropValue::Int(Some(i)) if *i < 0)) {
                        PropKind::I64
                    } else {
                        PropKind::U64
                    }
                }
            })
        })
        .collect()
}

/// One value in the column type [`column_kinds`] settled on, `NULL` included.
fn to_prop_value(kind: PropKind, value: PostgresPropValue) -> PropValue {
    match value {
        PostgresPropValue::Bool(v) => PropValue::Bool(v),
        PostgresPropValue::Float(v) => PropValue::F32(v),
        PostgresPropValue::Double(v) => PropValue::F64(v),
        PostgresPropValue::Text(v) => PropValue::Str(v),
        PostgresPropValue::Int(v) if kind == PropKind::U64 => {
            PropValue::U64(v.and_then(|i| u64::try_from(i).ok()))
        }
        PostgresPropValue::Int(v) => PropValue::I64(v),
    }
}

/// Converts a tile-space geometry into the `geo_types::Geometry<i32>` `mlt-core` accepts.
///
/// [`TileVertex::m`] is dropped, and a single-part multi-geometry collapses to its singular form
/// the way MVT encodes it.
fn to_encodable_geometry(geometry: TileGeometry) -> Geometry<i32> {
    match geometry {
        TileGeometry::Point(vertex) => Geometry::Point(Point(to_coord(vertex))),
        TileGeometry::LineString(vertices) => Geometry::LineString(to_line(vertices)),
        TileGeometry::Polygon(rings) => Geometry::Polygon(to_polygon(rings)),
        TileGeometry::MultiPoint(vertices) => match <[_; 1]>::try_from(vertices) {
            Ok([vertex]) => Geometry::Point(Point(to_coord(vertex))),
            Err(vertices) => Geometry::MultiPoint(MultiPoint(
                vertices.into_iter().map(|v| Point(to_coord(v))).collect(),
            )),
        },
        TileGeometry::MultiLineString(lines) => match <[_; 1]>::try_from(lines) {
            Ok([line]) => Geometry::LineString(to_line(line)),
            Err(lines) => {
                Geometry::MultiLineString(MultiLineString(lines.into_iter().map(to_line).collect()))
            }
        },
        TileGeometry::MultiPolygon(polygons) => match <[_; 1]>::try_from(polygons) {
            Ok([polygon]) => Geometry::Polygon(to_polygon(polygon)),
            Err(polygons) => {
                Geometry::MultiPolygon(MultiPolygon(polygons.into_iter().map(to_polygon).collect()))
            }
        },
        TileGeometry::GeometryCollection(parts) => Geometry::GeometryCollection(
            GeometryCollection(parts.into_iter().map(to_encodable_geometry).collect()),
        ),
    }
}

fn to_coord(vertex: TileVertex) -> Coord<i32> {
    Coord {
        x: vertex.x,
        y: vertex.y,
    }
}

fn to_line(vertices: Vec<TileVertex>) -> LineString<i32> {
    LineString(vertices.into_iter().map(to_coord).collect())
}

fn to_polygon(rings: Vec<Vec<TileVertex>>) -> Polygon<i32> {
    let mut rings = rings.into_iter().map(to_line);
    let exterior = rings.next().unwrap_or_else(|| LineString(Vec::new()));
    Polygon::new(exterior, rings.collect())
}

#[cfg(test)]
mod tests {
    use mlt_core::{Decoder, Layer, Parser};
    use rstest::rstest;

    use super::*;

    fn vertex(x: i32, y: i32, m: Option<f64>) -> TileVertex {
        TileVertex { x, y, m }
    }

    fn feature(properties: Vec<(&str, PostgresPropValue)>) -> PostgresFeature {
        PostgresFeature {
            id: None,
            geometry: TileGeometry::Point(vertex(1, 2, None)),
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
    #[case::boolean(PostgresPropValue::Bool(Some(true)), PropKind::Bool)]
    #[case::float(PostgresPropValue::Float(Some(1.5)), PropKind::F32)]
    #[case::double(PostgresPropValue::Double(Some(1.5)), PropKind::F64)]
    #[case::text(PostgresPropValue::Text(Some("x".to_owned())), PropKind::Str)]
    #[case::non_negative_integer(PostgresPropValue::Int(Some(7)), PropKind::U64)]
    #[case::negative_integer(PostgresPropValue::Int(Some(-7)), PropKind::I64)]
    fn a_column_takes_the_type_of_the_values_in_it(
        #[case] value: PostgresPropValue,
        #[case] expected: PropKind,
    ) {
        let features = vec![feature(vec![("p", value)])];
        assert_eq!(column_kinds(&features), vec![Some(expected)]);
    }

    #[test]
    fn one_negative_value_makes_the_whole_integer_column_signed() {
        let features = vec![
            feature(vec![("p", PostgresPropValue::Int(Some(7)))]),
            feature(vec![("p", PostgresPropValue::Int(None))]),
            feature(vec![("p", PostgresPropValue::Int(Some(-1)))]),
        ];
        assert_eq!(column_kinds(&features), vec![Some(PropKind::I64)]);
    }

    #[test]
    fn a_column_that_is_null_everywhere_is_left_out() {
        let features = vec![
            feature(vec![
                ("empty", PostgresPropValue::Int(None)),
                ("filled", PostgresPropValue::Text(Some("a".to_owned()))),
            ]),
            feature(vec![
                ("empty", PostgresPropValue::Int(None)),
                ("filled", PostgresPropValue::Text(None)),
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
    fn the_m_ordinate_is_dropped() {
        let measured = TileGeometry::LineString(vec![
            vertex(0, 0, Some(11.0)),
            vertex(1, 2, Some(12.0)),
            vertex(3, 4, None),
        ]);
        assert_eq!(
            to_encodable_geometry(measured),
            Geometry::LineString(LineString(vec![
                Coord { x: 0, y: 0 },
                Coord { x: 1, y: 2 },
                Coord { x: 3, y: 4 },
            ]))
        );
    }

    #[test]
    fn a_single_part_multi_geometry_collapses_the_way_mvt_encodes_it() {
        assert_eq!(
            to_encodable_geometry(TileGeometry::MultiPoint(vec![vertex(1, 2, Some(3.0))])),
            Geometry::Point(Point(Coord { x: 1, y: 2 }))
        );
        assert_eq!(
            to_encodable_geometry(TileGeometry::MultiLineString(vec![vec![
                vertex(0, 0, None),
                vertex(1, 1, None),
            ]])),
            Geometry::LineString(LineString(
                vec![Coord { x: 0, y: 0 }, Coord { x: 1, y: 1 },]
            ))
        );
        assert_eq!(
            to_encodable_geometry(TileGeometry::MultiPolygon(vec![vec![vec![
                vertex(0, 0, None),
                vertex(1, 0, None),
                vertex(1, 1, None),
                vertex(0, 0, None),
            ]]])),
            Geometry::Polygon(Polygon::new(
                LineString(vec![
                    Coord { x: 0, y: 0 },
                    Coord { x: 1, y: 0 },
                    Coord { x: 1, y: 1 },
                    Coord { x: 0, y: 0 },
                ]),
                Vec::new()
            ))
        );
    }

    #[test]
    fn a_multi_part_geometry_keeps_its_parts() {
        assert_eq!(
            to_encodable_geometry(TileGeometry::MultiPoint(vec![
                vertex(1, 2, None),
                vertex(3, 4, None),
            ])),
            Geometry::MultiPoint(MultiPoint(vec![
                Point(Coord { x: 1, y: 2 }),
                Point(Coord { x: 3, y: 4 }),
            ]))
        );
    }

    #[test]
    fn a_polygons_first_ring_is_its_exterior() {
        let ring = |size: i32| {
            vec![
                vertex(0, 0, None),
                vertex(size, 0, None),
                vertex(size, size, None),
                vertex(0, 0, None),
            ]
        };
        let Geometry::Polygon(polygon) =
            to_encodable_geometry(TileGeometry::Polygon(vec![ring(8), ring(2)]))
        else {
            panic!("a polygon must convert to a polygon");
        };
        assert_eq!(polygon.exterior().0.len(), 4);
        assert_eq!(polygon.interiors().len(), 1);
    }

    #[test]
    fn a_feature_id_and_its_properties_survive_the_encoding() {
        let features = vec![PostgresFeature {
            id: Some(42),
            geometry: TileGeometry::Point(vertex(10, 20, Some(1.0))),
            properties: vec![
                ("n".into(), PostgresPropValue::Int(Some(-5))),
                ("s".into(), PostgresPropValue::Text(Some("hi".to_owned()))),
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
        assert_eq!(
            feature.geometry(),
            &Geometry::Point(Point(Coord { x: 10, y: 20 }))
        );
        assert_eq!(
            feature.properties(),
            [
                PropValue::I32(Some(-5)),
                PropValue::Str(Some("hi".to_owned()))
            ]
        );
    }
}

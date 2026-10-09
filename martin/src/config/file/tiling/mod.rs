mod condition;
mod error;
mod primitives;
mod rule;
mod setting;
mod sources;
mod tile;
mod value;
mod zoom;

use std::num::NonZeroU32;

pub use condition::{Bound, Condition, GeometryType, NameMatch, PropertyTest, Range};
pub use error::TilingConfigError;
use indexmap::IndexMap;
pub use primitives::{Expr, Finite, Literal, NonEmpty, checked_map_with};
pub use rule::{Rule, RuleSettings, Rules};
use serde::{Deserialize, Deserializer, Serialize};
use setting::fixed_zoom_range;
pub use setting::{ByZoom, FromZoomSteps, Meters, PerFeature, PixelSetting, Pixels, ZoomSetting};
pub use sources::{
    Areas, CsvFiles, EngineFiles, EngineSource, GpkgFiles, Index, NoOptions, OsmPbf, OsmPbfFiles,
    Prefetch, RelationTags, ShapefileFiles,
};
pub use tile::{GridKeep, LabelGrid, LineLength, MergeLines, MergeMulti, MergePolygons, TileOps};
pub use value::{
    Attributes, Case, Cast, Columns, Computed, IdPolicy, Lookup, Match, PropertySelector, Ref,
    SortKey, Value, ValueSpec,
};
pub use zoom::{Zoom, ZoomRange};

use crate::config::file::{CollectUnrecognizedKeys, UnrecognizedKeys, UnrecognizedValues};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Layers(IndexMap<String, Layer>);

impl TryFrom<IndexMap<String, Layer>> for Layers {
    type Error = TilingConfigError;

    fn try_from(layers: IndexMap<String, Layer>) -> Result<Self, TilingConfigError> {
        if layers.is_empty() {
            return Err(TilingConfigError::NoLayers);
        }
        for (name, layer) in &layers {
            layer
                .check()
                .map_err(|e| TilingConfigError::InLayer(name.clone(), Box::new(e)))?;
        }
        Ok(Self(layers))
    }
}

impl<'de> Deserialize<'de> for Layers {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        checked_map_with(deserializer, |layers: IndexMap<String, Layer>| {
            Self::try_from(layers)
        })
    }
}

impl CollectUnrecognizedKeys for Layers {
    fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys) {
        for (name, layer) in &self.0 {
            let path = format!("{path}{name}.");
            layer.unrecognized.collect_unrecognized(&path, out);
            layer
                .rules
                .collect_unrecognized(&format!("{path}rules."), out);
            layer
                .tile
                .collect_unrecognized(&format!("{path}tile."), out);
        }
    }
}

impl CollectUnrecognizedKeys for Prefetch {
    fn collect_unrecognized(&self, _path: &str, _out: &mut UnrecognizedKeys) {}
}

impl Layers {
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Layer)> {
        self.0.iter().map(|(name, layer)| (name.as_str(), layer))
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Layer> {
        self.0.get(name)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#where: Option<Condition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<OutputGeometry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minzoom: Option<ZoomSetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maxzoom: Option<ZoomSetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extent: Option<NonZeroU32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip_geom: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simplify: Option<PixelSetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simplify_at_maxzoom: Option<Pixels>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_size: Option<PixelSetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_size_at_maxzoom: Option<Pixels>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub r#let: IndexMap<String, Computed>,
    #[serde(default, skip_serializing_if = "Attributes::is_all_properties")]
    pub attributes: Attributes,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Rules>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_by: Option<NonEmpty<SortKey>>,
    #[serde(default, skip_serializing_if = "IdPolicy::is_keep")]
    pub id: IdPolicy,
    #[serde(default, skip_serializing_if = "TileOps::is_empty")]
    pub tile: TileOps,
    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl Layer {
    fn check(&self) -> Result<(), TilingConfigError> {
        fixed_zoom_range(self.minzoom.as_ref(), self.maxzoom.as_ref()).map(|_| ())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputGeometry {
    Point,
    Line,
    Polygon,
    LabelPoint,
    Centroid,
    CentroidIfConvex,
    LineMidpoint,
    InnermostPoint,
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    pub(super) fn parse(yaml: &str) -> Layers {
        serde_saphyr::from_str(yaml).expect("parses")
    }

    pub(super) fn rejection(yaml: &str) -> String {
        let err = serde_saphyr::from_str::<Layers>(yaml)
            .expect_err("must not parse")
            .to_string();
        err.lines().next().unwrap_or_default().to_owned()
    }

    #[test]
    fn layers_keep_their_order() {
        let layers = parse(indoc! {"
            water: {}
            roads: {}
            buildings: {}
        "});
        assert_eq!(
            layers.names().collect::<Vec<_>>(),
            ["water", "roads", "buildings"]
        );
    }

    #[test]
    fn layers_cannot_be_empty() {
        insta::assert_snapshot!(rejection("{}"), @"`layers` needs at least one layer");
    }

    #[test]
    fn layers_cannot_repeat_a_name() {
        insta::assert_snapshot!(rejection("roads: {}\nroads: {}"), @"error: line 2 column 1: duplicate mapping key: roads, set DuplicateKeyPolicy in Options if acceptable");
    }

    #[test]
    fn a_layer_shapes_its_output_geometry() {
        let layers = parse(indoc! {"
            roads: { geometry: line }
            poi: { geometry: centroid_if_convex }
            water: {}
        "});
        let geometries: Vec<_> = layers.iter().map(|(name, l)| (name, l.geometry)).collect();
        assert_eq!(
            geometries,
            [
                ("roads", Some(OutputGeometry::Line)),
                ("poi", Some(OutputGeometry::CentroidIfConvex)),
                ("water", None),
            ]
        );
    }

    #[test]
    fn an_unknown_geometry_is_rejected() {
        insta::assert_snapshot!(rejection("roads: { geometry: curve }"), @"error: line 1 column 20: unknown variant `curve`, expected one of point, line, polygon, label_point, centroid, centroid_if_convex, line_midpoint, innermost_point");
    }

    #[test]
    fn a_layer_overrides_the_tiling_of_its_source() {
        let layers = parse("transportation: { extent: 512, buffer: 4, clip_geom: false }");
        let layer = layers.get("transportation").expect("layer exists");
        assert_eq!(layer.extent, NonZeroU32::new(512));
        assert_eq!(layer.buffer, Some(4));
        assert_eq!(layer.clip_geom, Some(false));
    }

    #[test]
    fn a_minzoom_beyond_the_maximum_is_rejected() {
        insta::assert_snapshot!(rejection("roads: { minzoom: 31 }"), @"error: line 1 column 10: invalid value: integer `31`, expected a zoom from 0 to 30");
    }

    #[test]
    fn a_minzoom_above_the_maxzoom_is_rejected() {
        insta::assert_snapshot!(rejection("roads: { minzoom: 12, maxzoom: 4 }"), @"layer `roads`: minzoom 12 is above maxzoom 4");
    }

    #[test]
    fn a_zero_extent_is_rejected() {
        insta::assert_snapshot!(rejection("roads: { extent: 0 }"), @"error: line 1 column 10: invalid value: integer `0`, expected a nonzero u32");
    }

    #[test]
    fn computed_values_and_sort_order_are_parsed() {
        let layers = parse(indoc! {r#"
            building:
              let:
                class: { match: [ { if: { highway: motorway }, value: motorway }, { else: minor } ] }
                height: { expr: "int(props.height)" }
                kind: highway
        "#});
        let computed = &layers.get("building").expect("layer exists").r#let;
        assert_eq!(
            computed.keys().collect::<Vec<_>>(),
            ["class", "height", "kind"]
        );
        assert_eq!(
            computed["class"],
            Computed(Value::Match(Match {
                cases: NonEmpty::new(Case {
                    when: Condition {
                        properties: IndexMap::from([(
                            "highway".to_owned(),
                            PropertyTest::OneOf(NonEmpty::new(Literal::String(
                                "motorway".to_owned()
                            ))),
                        )]),
                        ..Condition::default()
                    },
                    then: Value::Literal(Literal::String("motorway".to_owned())),
                }),
                otherwise: Some(Box::new(Value::Literal(Literal::String(
                    "minor".to_owned()
                )))),
            }))
        );
        assert_eq!(
            computed["height"],
            Computed(Value::Expr(
                Expr::new("int(props.height)").expect("not blank")
            ))
        );
        assert_eq!(
            computed["kind"],
            Computed(Value::Copy {
                from: Ref::Property("highway".to_owned()),
                otherwise: None,
            })
        );
    }

    #[test]
    fn sort_keys_are_parsed_in_order() {
        let layers = parse(indoc! {r#"
            building:
              sort_by: [ { expr: "props.z_order" }, { from: let.height, desc: true }, class ]
        "#});
        let keys = layers
            .get("building")
            .expect("layer exists")
            .sort_by
            .as_ref()
            .expect("sort_by is set");
        let expected = [
            SortKey {
                value: Computed(Value::Expr(Expr::new("props.z_order").expect("not blank"))),
                descending: false,
            },
            SortKey {
                value: Computed(Value::Copy {
                    from: Ref::Let("height".to_owned()),
                    otherwise: None,
                }),
                descending: true,
            },
            SortKey {
                value: Computed(Value::Copy {
                    from: Ref::Property("class".to_owned()),
                    otherwise: None,
                }),
                descending: false,
            },
        ];
        assert_eq!(keys.iter().cloned().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn an_id_is_kept_dropped_or_computed() {
        let layers = parse(indoc! {"
            omitted: {}
            kept: { id: keep }
            dropped: { id: drop }
            computed: { id: { expr: 'props.osm_id * 10' } }
        "});
        let ids: Vec<_> = layers
            .iter()
            .map(|(name, l)| (name, l.id.clone()))
            .collect();
        assert_eq!(
            ids,
            [
                ("omitted", IdPolicy::Keep),
                ("kept", IdPolicy::Keep),
                ("dropped", IdPolicy::Drop),
                (
                    "computed",
                    IdPolicy::Expr(Expr::new("props.osm_id * 10").expect("not blank"))
                ),
            ]
        );
    }

    #[test]
    fn minzoom_cannot_change_with_the_zoom() {
        insta::assert_snapshot!(
            rejection("roads: { minzoom: { 0: 2, 10: 4 } }"),
            @"error: line 1 column 19: a zoom cannot change with the zoom"
        );
    }

    #[test]
    fn an_empty_match_is_rejected() {
        insta::assert_snapshot!(
            rejection("roads: { minzoom: { match: [] } }"),
            @"error: line 1 column 28: `match` needs at least one `if` case"
        );
    }

    #[test]
    fn expr_and_lookup_cannot_be_mixed() {
        insta::assert_snapshot!(
            rejection("roads: { minzoom: { expr: x, lookup: y } }"),
            @"error: line 1 column 19: pick one of `match`, `lookup` or `expr`"
        );
    }

    #[test]
    fn a_lookup_needs_a_map() {
        insta::assert_snapshot!(
            rejection("roads: { minzoom: { lookup: kind } }"),
            @"error: line 1 column 19: `lookup` needs a `map`"
        );
    }

    #[test]
    fn else_needs_a_lookup() {
        insta::assert_snapshot!(
            rejection("roads: { minzoom: { else: 4 } }"),
            @"error: line 1 column 19: `else` goes with `lookup`; in a `match`, write it as its last case"
        );
    }

    #[test]
    fn a_negative_simplify_is_rejected() {
        insta::assert_snapshot!(
            rejection("roads: { simplify: -1 }"),
            @"error: line 1 column 10: invalid value: floating point `-1.0`, expected a finite number of pixels, 0 or more"
        );
    }

    #[test]
    fn an_empty_simplify_is_rejected() {
        insta::assert_snapshot!(
            rejection("roads: { simplify: {} }"),
            @"error: line 1 column 20: needs zoom steps such as `{ 0: 2, 11: 0 }`, or one of `match`, `lookup` or `expr`"
        );
    }

    #[test]
    fn simplify_zoom_steps_cannot_exceed_the_deepest_zoom() {
        insta::assert_snapshot!(
            rejection("roads: { simplify: { 0: 2, 31: 0 } }"),
            @"error: line 1 column 28: invalid value: integer `31`, expected a zoom from 0 to 30"
        );
    }

    #[test]
    fn simplify_zoom_steps_cannot_be_mixed_with_match() {
        insta::assert_snapshot!(
            rejection("roads: { simplify: { 0: 2, match: [] } }"),
            @"error: line 1 column 20: zoom steps cannot be mixed with `match`"
        );
    }

    #[test]
    fn a_let_value_cannot_carry_a_zoom() {
        insta::assert_snapshot!(
            rejection("roads: { let: { x: { from: a, minzoom: 4 } } }"),
            @"error: line 1 column 20: `minzoom` only applies to a whole attribute"
        );
    }

    #[test]
    fn a_let_value_cannot_be_descending() {
        insta::assert_snapshot!(
            rejection("roads: { let: { x: { from: a, desc: true } } }"),
            @"error: line 1 column 20: `desc` only applies to a key of `sort_by`"
        );
    }

    #[test]
    fn an_unknown_id_policy_is_rejected() {
        insta::assert_snapshot!(
            rejection("roads: { id: none }"),
            @"error: line 1 column 10: unknown variant `none`, expected one of keep, drop"
        );
    }

    #[test]
    fn an_empty_sort_by_is_rejected() {
        insta::assert_snapshot!(
            rejection("roads: { sort_by: [] }"),
            @"error: line 1 column 10: invalid length 0, expected at least one item"
        );
    }
}

#[cfg(test)]
mod proptests {
    use std::fmt::Debug;
    use std::num::NonZeroU32;

    use indexmap::IndexMap;
    use proptest::collection::{btree_map, btree_set, vec};
    use proptest::prelude::*;

    use super::*;
    use crate::config::file::UnrecognizedValues;

    fn name() -> impl Strategy<Value = String> + Clone {
        "[a-z][a-z0-9_:]{0,6}"
    }

    fn text() -> impl Strategy<Value = String> + Clone {
        prop_oneof![
            "[a-zA-Z0-9 _:%.-]{0,8}",
            Just("yes".to_owned()),
            Just("no".to_owned()),
            Just("true".to_owned()),
            Just("null".to_owned()),
            Just("~".to_owned()),
            Just("1".to_owned()),
            Just("0.5".to_owned()),
            Just("let.class".to_owned()),
            Just("name:*".to_owned()),
        ]
    }

    fn non_empty<T: Debug>(
        item: impl Strategy<Value = T> + Clone,
    ) -> impl Strategy<Value = NonEmpty<T>> + Clone {
        vec(item, 1..4).prop_map(|items| NonEmpty::try_from_vec(items).expect("1..4 items"))
    }

    fn map_of<V: Debug>(
        key: impl Strategy<Value = String> + Clone,
        value: impl Strategy<Value = V> + Clone,
        size: std::ops::Range<usize>,
    ) -> impl Strategy<Value = IndexMap<String, V>> + Clone {
        vec((key, value), size).prop_map(|entries| entries.into_iter().collect())
    }

    fn finite() -> impl Strategy<Value = Finite> + Clone {
        any::<f64>().prop_filter_map("finite", Finite::new)
    }

    fn literal() -> impl Strategy<Value = Literal> + Clone {
        prop_oneof![
            any::<bool>().prop_map(Literal::Bool),
            any::<i64>().prop_map(Literal::Int),
            finite().prop_map(Literal::Float),
            text().prop_map(Literal::String),
        ]
    }

    fn expr() -> impl Strategy<Value = Expr> + Clone {
        "[a-z.() =<>'0-9]{1,12}".prop_filter_map("not blank", Expr::new)
    }

    fn reference() -> impl Strategy<Value = Ref> + Clone {
        prop_oneof![name().prop_map(Ref::Property), name().prop_map(Ref::Let)]
    }

    fn zoom() -> impl Strategy<Value = Zoom> + Clone {
        (0..=30_u8).prop_map(|z| Zoom::new(z).expect("0..=30"))
    }

    fn zoom_range() -> impl Strategy<Value = ZoomRange> + Clone {
        (proptest::option::of(zoom()), proptest::option::of(zoom()))
            .prop_filter_map("min <= max", |(min, max)| ZoomRange::new(min, max).ok())
    }

    fn pixels() -> impl Strategy<Value = Pixels> + Clone {
        (0.0..1e6_f64).prop_map(|v| Pixels::new(v).expect("non-negative"))
    }

    fn meters() -> impl Strategy<Value = Meters> + Clone {
        (0.0..1e7_f64).prop_map(|v| Meters::new(v).expect("non-negative"))
    }

    fn by_zoom<U: Clone + Debug>(
        unit: impl Strategy<Value = U> + Clone,
    ) -> impl Strategy<Value = ByZoom<U>> + Clone {
        prop_oneof![
            unit.clone().prop_map(ByZoom::Constant),
            btree_map(zoom(), unit, 1..4).prop_map(ByZoom::Steps),
        ]
    }

    fn bound() -> impl Strategy<Value = Bound> + Clone {
        (finite(), any::<bool>()).prop_map(|(value, inclusive)| Bound { value, inclusive })
    }

    fn range() -> impl Strategy<Value = Range> + Clone {
        prop_oneof![
            bound().prop_map(Range::From),
            bound().prop_map(Range::To),
            (bound(), bound()).prop_filter_map("non-empty", |(a, b)| {
                let nonempty =
                    a.value < b.value || (a.value == b.value && a.inclusive && b.inclusive);
                nonempty.then_some(Range::Between(a, b))
            }),
        ]
    }

    fn property_test() -> impl Strategy<Value = PropertyTest> + Clone {
        prop_oneof![
            non_empty(literal()).prop_map(PropertyTest::OneOf),
            text().prop_map(PropertyTest::Like),
            range().prop_map(PropertyTest::Range),
        ]
    }

    fn geometry_type() -> impl Strategy<Value = GeometryType> + Clone {
        prop_oneof![
            Just(GeometryType::Point),
            Just(GeometryType::Line),
            Just(GeometryType::Polygon),
        ]
    }

    fn name_match() -> impl Strategy<Value = NameMatch> + Clone {
        prop_oneof![
            non_empty(name()).prop_map(NameMatch::OneOf),
            text().prop_map(NameMatch::Like),
        ]
    }

    fn condition() -> impl Strategy<Value = Condition> + Clone {
        let leaf = (
            map_of(name(), property_test(), 0..3),
            proptest::option::of(non_empty(name())),
            proptest::option::of(non_empty(name())),
            proptest::option::of(expr()),
            proptest::option::of(non_empty(geometry_type())),
            proptest::option::of(name_match()),
        )
            .prop_map(
                |(properties, has, missing, expr, geometry, source_layer)| Condition {
                    properties,
                    has,
                    missing,
                    expr,
                    geometry,
                    source_layer,
                    ..Condition::default()
                },
            )
            .prop_filter("non-empty", |c| *c != Condition::default());
        leaf.prop_recursive(2, 8, 2, |inner| {
            (
                inner.clone(),
                proptest::option::of(non_empty(inner.clone())),
                proptest::option::of(non_empty(inner.clone())),
                proptest::option::of(inner),
            )
                .prop_map(|(base, any, all, not)| Condition {
                    any: any.or(base.any.clone()),
                    all: all.or(base.all.clone()),
                    not: not.map(Box::new).or(base.not.clone()),
                    ..base
                })
        })
    }

    fn value() -> impl Strategy<Value = Value> + Clone {
        let leaf = prop_oneof![
            literal().prop_map(Value::Literal),
            reference().prop_map(|from| Value::Copy {
                from,
                otherwise: None
            }),
            non_empty(reference()).prop_map(Value::Coalesce),
            map_of(name(), reference(), 1..3).prop_map(Value::Struct),
            expr().prop_map(Value::Expr),
        ];
        leaf.prop_recursive(2, 8, 2, |inner| {
            prop_oneof![
                (reference(), inner.clone()).prop_map(|(from, otherwise)| Value::Copy {
                    from,
                    otherwise: Some(Box::new(otherwise)),
                }),
                (
                    non_empty(
                        (condition(), inner.clone()).prop_map(|(when, then)| Case { when, then })
                    ),
                    proptest::option::of(inner.clone()),
                )
                    .prop_map(|(cases, otherwise)| Value::Match(Match {
                        cases,
                        otherwise: otherwise.map(Box::new),
                    })),
                (
                    reference(),
                    map_of(text(), inner.clone(), 1..3),
                    proptest::option::of(inner)
                )
                    .prop_map(|(subject, table, otherwise)| Value::Lookup(
                        Lookup {
                            subject,
                            table,
                            otherwise: otherwise.map(Box::new),
                        }
                    )),
            ]
        })
    }

    fn cast() -> impl Strategy<Value = Cast> + Clone {
        prop_oneof![
            Just(Cast::Int),
            Just(Cast::Float),
            Just(Cast::String),
            Just(Cast::Bool)
        ]
    }

    fn value_spec() -> impl Strategy<Value = ValueSpec> + Clone {
        (
            value(),
            proptest::option::of(cast()),
            proptest::option::of(literal()),
            zoom_range(),
            proptest::option::of(condition()),
        )
            .prop_map(|(value, cast, null_if, zooms, r#where)| ValueSpec {
                value,
                cast,
                null_if,
                zooms,
                r#where,
            })
    }

    fn per_feature<T: Clone + Debug + 'static>(
        leaf: impl Strategy<Value = T> + Clone + 'static,
    ) -> impl Strategy<Value = PerFeature<T>> + Clone {
        prop_oneof![
            leaf.prop_map(PerFeature::Fixed),
            expr().prop_map(PerFeature::Expr)
        ]
        .prop_recursive(2, 8, 3, |inner| {
            prop_oneof![
                (
                    non_empty(
                        (condition(), inner.clone()).prop_map(|(when, then)| Case { when, then })
                    ),
                    proptest::option::of(inner.clone()),
                )
                    .prop_map(|(cases, otherwise)| PerFeature::Match(Match {
                        cases,
                        otherwise: otherwise.map(Box::new),
                    })),
                (
                    reference(),
                    map_of(text(), inner.clone(), 1..3),
                    proptest::option::of(inner)
                )
                    .prop_map(|(subject, table, otherwise)| PerFeature::Lookup(
                        Lookup {
                            subject,
                            table,
                            otherwise: otherwise.map(Box::new),
                        }
                    )),
            ]
        })
    }

    fn fixed_zooms_in_order(min: Option<&ZoomSetting>, max: Option<&ZoomSetting>) -> bool {
        let fixed = |z: Option<&ZoomSetting>| z.and_then(PerFeature::fixed).copied();
        ZoomRange::new(fixed(min), fixed(max)).is_ok()
    }

    fn property_selector() -> impl Strategy<Value = PropertySelector> + Clone {
        prop_oneof![
            name().prop_map(PropertySelector::Named),
            name().prop_map(PropertySelector::Prefixed),
        ]
    }

    fn attributes() -> impl Strategy<Value = Attributes> + Clone {
        prop_oneof![
            Just(Attributes::AllProperties),
            Just(Attributes::None),
            non_empty(property_selector()).prop_map(Attributes::Properties),
            (map_of(name(), value_spec(), 0..3), btree_set(name(), 0..3))
                .prop_filter("non-empty", |(computed, prefixes)| {
                    !computed.is_empty() || !prefixes.is_empty()
                })
                .prop_map(|(computed, prefixes)| Attributes::Columns(Columns {
                    computed,
                    copied_prefixes: prefixes.into_iter().collect(),
                })),
        ]
    }

    fn rule_settings() -> impl Strategy<Value = RuleSettings> + Clone {
        (
            proptest::option::of(per_feature(zoom())),
            proptest::option::of(per_feature(zoom())),
            proptest::option::of(per_feature(by_zoom(pixels()))),
            proptest::option::of(per_feature(by_zoom(pixels()))),
            map_of(name(), value_spec(), 0..2),
        )
            .prop_filter("fixed minzoom <= maxzoom", |(min, max, ..)| {
                fixed_zooms_in_order(min.as_ref(), max.as_ref())
            })
            .prop_map(
                |(minzoom, maxzoom, simplify, min_size, attributes)| RuleSettings {
                    minzoom,
                    maxzoom,
                    simplify,
                    min_size,
                    attributes,
                    unrecognized: UnrecognizedValues::default(),
                },
            )
    }

    fn rules() -> impl Strategy<Value = Rules> + Clone {
        (
            non_empty(
                (condition(), rule_settings()).prop_map(|(when, settings)| Rule { when, settings }),
            ),
            proptest::option::of(rule_settings()),
        )
            .prop_map(|(cases, fallback)| Rules { cases, fallback })
    }

    fn tile_ops() -> impl Strategy<Value = TileOps> + Clone {
        let merge_lines = (
            vec(name(), 0..2),
            proptest::option::of(prop_oneof![
                by_zoom(pixels()).prop_map(LineLength::Pixels),
                by_zoom(meters()).prop_map(LineLength::Meters),
            ]),
            proptest::option::of(pixels()),
            proptest::option::of(condition()),
            zoom_range(),
        )
            .prop_map(
                |(by, min_length, simplify, except_where, zooms)| MergeLines {
                    by,
                    min_length,
                    simplify,
                    except_where,
                    zooms,
                    unrecognized: UnrecognizedValues::default(),
                },
            );
        let merge_polygons = (
            vec(name(), 0..2),
            proptest::option::of(by_zoom(pixels())),
            proptest::option::of(pixels()),
            zoom_range(),
        )
            .prop_map(|(by, min_area, gap, zooms)| MergePolygons {
                by,
                min_area,
                gap,
                zooms,
                unrecognized: UnrecognizedValues::default(),
            });
        let positive = (1..1000_u32).prop_map(|n| NonZeroU32::new(n).expect("1.."));
        let label_grid = (
            positive.clone(),
            prop_oneof![
                positive.clone().prop_map(GridKeep::Best),
                name().prop_map(|rank_attribute| GridKeep::All { rank_attribute }),
                (positive.clone(), name()).prop_map(|(best, rank_attribute)| GridKeep::Ranked {
                    best,
                    rank_attribute
                }),
            ],
            zoom_range(),
        )
            .prop_map(|(size, keep, zooms)| LabelGrid {
                size,
                keep,
                zooms,
                unrecognized: UnrecognizedValues::default(),
            });
        (
            proptest::option::of(merge_lines),
            proptest::option::of(merge_polygons),
            proptest::option::of(zoom_range().prop_map(|zooms| MergeMulti {
                zooms,
                unrecognized: UnrecognizedValues::default(),
            })),
            proptest::option::of(label_grid),
            proptest::option::of(non_empty(name())),
            proptest::option::of(positive),
        )
            .prop_map(
                |(merge_lines, merge_polygons, merge_multi, label_grid, dedup_by, limit)| TileOps {
                    merge_lines,
                    merge_polygons,
                    merge_multi,
                    label_grid,
                    dedup_by,
                    limit,
                    unrecognized: UnrecognizedValues::default(),
                },
            )
    }

    fn output_geometry() -> impl Strategy<Value = OutputGeometry> + Clone {
        prop_oneof![
            Just(OutputGeometry::Point),
            Just(OutputGeometry::Line),
            Just(OutputGeometry::Polygon),
            Just(OutputGeometry::LabelPoint),
            Just(OutputGeometry::Centroid),
            Just(OutputGeometry::CentroidIfConvex),
            Just(OutputGeometry::LineMidpoint),
            Just(OutputGeometry::InnermostPoint),
        ]
    }

    fn id_policy() -> impl Strategy<Value = IdPolicy> + Clone {
        prop_oneof![
            Just(IdPolicy::Keep),
            Just(IdPolicy::Drop),
            expr().prop_map(IdPolicy::Expr)
        ]
    }

    fn layer() -> impl Strategy<Value = Layer> + Clone {
        let selection = (
            proptest::option::of(condition()),
            proptest::option::of(output_geometry()),
            proptest::option::of(per_feature(zoom())),
            proptest::option::of(per_feature(zoom())),
        );
        let tiling = (
            proptest::option::of((1..8192_u32).prop_map(|n| NonZeroU32::new(n).expect("1.."))),
            proptest::option::of(any::<u32>()),
            proptest::option::of(any::<bool>()),
            proptest::option::of(per_feature(by_zoom(pixels()))),
            proptest::option::of(pixels()),
            proptest::option::of(per_feature(by_zoom(pixels()))),
            proptest::option::of(pixels()),
        );
        let output = (
            map_of(name(), value().prop_map(Computed), 0..2),
            attributes(),
            proptest::option::of(rules()),
            proptest::option::of(non_empty((value(), any::<bool>()).prop_map(
                |(v, descending)| SortKey {
                    value: Computed(v),
                    descending,
                },
            ))),
            id_policy(),
            tile_ops(),
        );
        (selection, tiling, output)
            .prop_filter("fixed minzoom <= maxzoom", |((_, _, min, max), ..)| {
                fixed_zooms_in_order(min.as_ref(), max.as_ref())
            })
            .prop_map(
                |(
                    (r#where, geometry, minzoom, maxzoom),
                    (
                        extent,
                        buffer,
                        clip_geom,
                        simplify,
                        simplify_at_maxzoom,
                        min_size,
                        min_size_at_maxzoom,
                    ),
                    (r#let, attributes, rules, sort_by, id, tile),
                )| Layer {
                    r#where,
                    geometry,
                    minzoom,
                    maxzoom,
                    extent,
                    buffer,
                    clip_geom,
                    simplify,
                    simplify_at_maxzoom,
                    min_size,
                    min_size_at_maxzoom,
                    r#let,
                    attributes,
                    rules,
                    sort_by,
                    id,
                    tile,
                    unrecognized: UnrecognizedValues::default(),
                },
            )
    }

    fn layers() -> impl Strategy<Value = Layers> + Clone {
        map_of(name(), layer(), 1..3)
            .prop_map(|layers| Layers::try_from(layers).expect("1..3 layers"))
    }

    #[test]
    fn every_config_survives_a_round_trip_through_yaml() {
        let deep_enough_for_unoptimized_builds = 16 << 20;
        std::thread::Builder::new()
            .stack_size(deep_enough_for_unoptimized_builds)
            .spawn(|| {
                proptest!(|(layers in layers())| {
                    let yaml = serde_saphyr::to_string(&layers).expect("serializes");
                    let parsed: Layers = serde_saphyr::from_str(&yaml)
                        .map_err(|e| TestCaseError::fail(format!("{e}\n{yaml}")))?;
                    prop_assert_eq!(parsed, layers, "{}", yaml);
                });
            })
            .expect("spawns")
            .join()
            .expect("every case passes");
    }
}

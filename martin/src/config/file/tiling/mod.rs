mod condition;
mod error;
mod primitives;
mod zoom;

use std::num::NonZeroU32;

pub use condition::{Bound, Condition, GeometryType, NameMatch, PropertyTest, Range};
pub use error::TilingConfigError;
use indexmap::IndexMap;
use primitives::checked_map_with;
pub use primitives::{Expr, Finite, Literal, NonEmpty};
use serde::{Deserialize, Deserializer, Serialize};
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
        }
    }
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
    pub minzoom: Option<Zoom>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maxzoom: Option<Zoom>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extent: Option<NonZeroU32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip_geom: Option<bool>,
    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl Layer {
    fn check(&self) -> Result<(), TilingConfigError> {
        ZoomRange::new(self.minzoom, self.maxzoom).map(|_| ())
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
}

#[cfg(test)]
mod proptests {
    use std::fmt::Debug;
    use std::num::NonZeroU32;

    use indexmap::IndexMap;
    use proptest::collection::vec;
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

    fn zoom() -> impl Strategy<Value = Zoom> + Clone {
        (0..=30_u8).prop_map(|z| Zoom::new(z).expect("0..=30"))
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

    fn layer() -> impl Strategy<Value = Layer> + Clone {
        let selection = (
            proptest::option::of(condition()),
            proptest::option::of(output_geometry()),
            proptest::option::of(zoom()),
            proptest::option::of(zoom()),
        );
        let tiling = (
            proptest::option::of((1..8192_u32).prop_map(|n| NonZeroU32::new(n).expect("1.."))),
            proptest::option::of(any::<u32>()),
            proptest::option::of(any::<bool>()),
        );
        (selection, tiling)
            .prop_filter("minzoom <= maxzoom", |((_, _, min, max), ..)| {
                ZoomRange::new(*min, *max).is_ok()
            })
            .prop_map(
                |((r#where, geometry, minzoom, maxzoom), (extent, buffer, clip_geom))| Layer {
                    r#where,
                    geometry,
                    minzoom,
                    maxzoom,
                    extent,
                    buffer,
                    clip_geom,
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

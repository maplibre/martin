mod error;
mod primitives;
mod rule;
mod setting;
mod tile;
mod value;
mod zoom;

use std::num::{NonZeroU8, NonZeroU32};

pub use error::TilingConfigError;
use indexmap::IndexMap;
pub use primitives::{Expr, Finite, Literal, NonEmpty, checked_map_with};
pub use rule::{Rule, RuleSettings, Rules};
use serde::{Deserialize, Deserializer, Serialize};
pub use setting::{ByZoom, Meters, PerFeature, Pixels, ZoomSetting};
pub use tile::{GridKeep, LabelGrid, LineLength, MergeLines, MergeMulti, MergePolygons, TileOps};
pub use value::{Attributes, Columns, IdPolicy, PropertySelector, SortKey, Value, ValueSpec};
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
            layer.check(name)?;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Prefetch(NonZeroU8);

impl Prefetch {
    #[must_use]
    pub fn tiles_per_side(self) -> u8 {
        self.0.get()
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

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub r#where: Option<Expr>,
    pub geometry: Option<OutputGeometry>,
    pub minzoom: Option<ZoomSetting>,
    pub maxzoom: Option<ZoomSetting>,
    pub extent: Option<NonZeroU32>,
    pub buffer: Option<u32>,
    pub clip_geom: Option<bool>,
    pub simplify: Option<Pixels>,
    simplify_at_maxzoom: Option<Pixels>,
    pub min_size: Option<Pixels>,
    min_size_at_maxzoom: Option<Pixels>,
    #[serde(default, skip_serializing_if = "Attributes::is_all_properties")]
    pub attributes: Attributes,
    pub rules: Option<Rules>,
    pub sort_by: Option<NonEmpty<SortKey>>,
    #[serde(default, skip_serializing_if = "IdPolicy::is_keep")]
    pub id: IdPolicy,
    #[serde(default, skip_serializing_if = "TileOps::is_empty")]
    pub tile: TileOps,
    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl Layer {
    #[must_use]
    pub fn simplify_at_maxzoom(&self) -> Option<Pixels> {
        self.simplify_at_maxzoom.or(self.simplify)
    }

    #[must_use]
    pub fn min_size_at_maxzoom(&self) -> Option<Pixels> {
        self.min_size_at_maxzoom.or(self.min_size)
    }

    fn check(&self, name: &str) -> Result<(), TilingConfigError> {
        let fixed = |setting: Option<&ZoomSetting>| setting.and_then(PerFeature::fixed).copied();
        match (fixed(self.minzoom.as_ref()), fixed(self.maxzoom.as_ref())) {
            (Some(min), Some(max)) if min > max => {
                Err(TilingConfigError::LayerMinzoomAboveMaxzoom {
                    layer: name.to_owned(),
                    min: min.get(),
                    max: max.get(),
                })
            }
            _ => Ok(()),
        }
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
    fn a_layer_selects_features_with_an_expression() {
        let layers = parse(indoc! {r#"
            roads:
              where: "highway in ['motorway', 'trunk'] && !(access in ['private', 'no'])"
        "#});
        assert_eq!(
            layers.get("roads").expect("layer exists").r#where,
            Some(
                Expr::new("highway in ['motorway', 'trunk'] && !(access in ['private', 'no'])")
                    .expect("valid CEL")
            )
        );
    }

    #[test]
    fn an_invalid_where_is_rejected() {
        insta::assert_snapshot!(
            rejection("roads: { where: \"highway in ['motorway'\" }"),
            @"error: line 1 column 17: `highway in ['motorway'` is not a valid CEL expression: Syntax error: mismatched input '<EOF>' expecting {']', ','}"
        );
    }

    #[test]
    fn sort_keys_are_parsed_in_order() {
        let layers = parse(indoc! {r#"
            building:
              sort_by: [ "z_order", { expr: "int(height)", desc: true } ]
        "#});
        let keys = layers
            .get("building")
            .expect("layer exists")
            .sort_by
            .as_ref()
            .expect("sort_by is set");
        let expected = [
            SortKey {
                expr: Expr::new("z_order").expect("valid CEL"),
                descending: false,
            },
            SortKey {
                expr: Expr::new("int(height)").expect("valid CEL"),
                descending: true,
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
            computed: { id: { expr: 'osm_id * 10' } }
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
                    IdPolicy::Expr(Expr::new("osm_id * 10").expect("valid CEL"))
                ),
            ]
        );
    }

    #[test]
    fn minzoom_cannot_change_with_the_zoom() {
        insta::assert_snapshot!(
            rejection("roads: { minzoom: { 0: 2, 10: 4 } }"),
            @"error: line 1 column 19: invalid type: map, expected a number or an expression"
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
    fn simplify_cannot_change_with_the_zoom() {
        insta::assert_snapshot!(
            rejection("roads: { simplify: { 0: 2, 11: 0 } }"),
            @"error: line 1 column 20: invalid type: map, expected a finite number of pixels, 0 or more"
        );
    }

    #[test]
    fn min_size_cannot_change_with_the_zoom() {
        insta::assert_snapshot!(
            rejection("roads: { min_size: { 0: 2, 11: 0 } }"),
            @"error: line 1 column 20: invalid type: map, expected a finite number of pixels, 0 or more"
        );
    }

    #[test]
    fn empty_zoom_steps_are_rejected() {
        insta::assert_snapshot!(
            rejection("roads: { tile: { merge_polygons: { min_area: {} } } }"),
            @"error: line 1 column 46: invalid length 0, expected at least one zoom step"
        );
    }

    #[test]
    fn zoom_steps_cannot_exceed_the_deepest_zoom() {
        insta::assert_snapshot!(
            rejection("roads: { tile: { merge_polygons: { min_area: { 0: 2, 31: 0 } } } }"),
            @"error: line 1 column 54: invalid value: integer `31`, expected a zoom from 0 to 30"
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

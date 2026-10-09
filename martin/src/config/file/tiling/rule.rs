use std::fmt;

use indexmap::IndexMap;
use serde::de::value::SeqAccessDeserializer;
use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::error::TilingConfigError;
use super::primitives::{Expr, NonEmpty};
use super::setting::{PixelSetting, ZoomSetting, fixed_zoom_range};
use super::value::ValueSpec;
use crate::config::file::{CollectUnrecognizedKeys, UnrecognizedKeys, UnrecognizedValues};

#[derive(Clone, Debug, PartialEq)]
pub struct Rules {
    pub cases: NonEmpty<Rule>,
    pub fallback: Option<RuleSettings>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub when: Expr,
    pub settings: RuleSettings,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuleSettings {
    pub minzoom: Option<ZoomSetting>,
    pub maxzoom: Option<ZoomSetting>,
    pub simplify: Option<PixelSetting>,
    pub min_size: Option<PixelSetting>,
    pub attributes: IndexMap<String, ValueSpec>,
    pub unrecognized: UnrecognizedValues,
}

#[derive(Default, Serialize, Deserialize)]
struct RawRule {
    #[serde(default, rename = "where", skip_serializing_if = "Option::is_none")]
    when: Option<Expr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    minzoom: Option<ZoomSetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    maxzoom: Option<ZoomSetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    simplify: Option<PixelSetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    min_size: Option<PixelSetting>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    attributes: IndexMap<String, ValueSpec>,
    #[serde(flatten, skip_serializing)]
    unrecognized: UnrecognizedValues,
}

impl RawRule {
    fn split(self) -> Result<(Option<Expr>, RuleSettings), TilingConfigError> {
        fixed_zoom_range(self.minzoom.as_ref(), self.maxzoom.as_ref())?;
        let settings = RuleSettings {
            minzoom: self.minzoom,
            maxzoom: self.maxzoom,
            simplify: self.simplify,
            min_size: self.min_size,
            attributes: self.attributes,
            unrecognized: self.unrecognized,
        };
        Ok((self.when, settings))
    }

    fn join(when: Option<&Expr>, settings: &RuleSettings) -> Self {
        Self {
            when: when.cloned(),
            minzoom: settings.minzoom.clone(),
            maxzoom: settings.maxzoom.clone(),
            simplify: settings.simplify.clone(),
            min_size: settings.min_size.clone(),
            attributes: settings.attributes.clone(),
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

impl<'de> Deserialize<'de> for Rules {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RulesVisitor;

        impl<'de> Visitor<'de> for RulesVisitor {
            type Value = Rules;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a list of rules, each with a `where`, except perhaps the last")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Rules, A::Error> {
                let raw = Vec::<RawRule>::deserialize(SeqAccessDeserializer::new(seq))?;
                let mut cases = Vec::with_capacity(raw.len());
                let mut fallback = None;
                for rule in raw {
                    if fallback.is_some() {
                        return Err(de::Error::custom(TilingConfigError::CatchAllNotLast));
                    }
                    match rule.split().map_err(de::Error::custom)? {
                        (Some(when), settings) => cases.push(Rule { when, settings }),
                        (None, settings) => fallback = Some(settings),
                    }
                }
                let cases = NonEmpty::try_from_vec(cases)
                    .ok_or_else(|| de::Error::custom(TilingConfigError::NoConditionalRule))?;
                Ok(Rules { cases, fallback })
            }
        }

        deserializer.deserialize_seq(RulesVisitor)
    }
}

impl Serialize for Rules {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let cases = self
            .cases
            .iter()
            .map(|r| RawRule::join(Some(&r.when), &r.settings));
        let fallback = self.fallback.iter().map(|s| RawRule::join(None, s));
        serializer.collect_seq(cases.chain(fallback))
    }
}

impl CollectUnrecognizedKeys for Rules {
    fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys) {
        let base = path.strip_suffix('.').unwrap_or(path);
        let settings = self.cases.iter().map(|r| &r.settings).chain(&self.fallback);
        for (index, settings) in settings.enumerate() {
            settings
                .unrecognized
                .collect_unrecognized(&format!("{base}[{index}]."), out);
        }
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::{Rule, RuleSettings, Rules};
    use crate::config::file::tiling::setting::{ByZoom, PerFeature, Pixels};
    use crate::config::file::tiling::tests::{parse, rejection};
    use crate::config::file::tiling::{Expr, Literal, NonEmpty, Value, Zoom};

    fn fixed_zoom(z: u8) -> PerFeature<Zoom> {
        PerFeature::Fixed(Zoom::new(z).expect("valid zoom"))
    }

    fn px(value: f64) -> Pixels {
        Pixels::new(value).expect("valid pixels")
    }

    fn expr(source: &str) -> Expr {
        Expr::new(source).expect("valid CEL")
    }

    #[test]
    fn rules_pick_the_first_match_and_may_end_in_a_catch_all() {
        let layers = parse(indoc! {"
            poi:
              rules:
                - where: \"amenity in ['university', 'college']\"
                  minzoom: 10
                  min_size: { 0: 80, 13: 0 }
                - where: \"railway in ['station', 'halt']\"
                  minzoom: 12
                  attributes: { class: { value: railway } }
                - minzoom: 14
        "});
        let mut rules = layers
            .get("poi")
            .and_then(|layer| layer.rules.clone())
            .expect("poi has rules");
        let class = rules
            .cases
            .iter()
            .nth(1)
            .expect("two rules")
            .settings
            .attributes["class"]
            .clone();
        assert_eq!(
            class.value,
            Value::Literal(Literal::String("railway".to_owned()))
        );
        rules.cases = NonEmpty::try_from_vec(
            rules
                .cases
                .iter()
                .cloned()
                .map(|mut rule| {
                    rule.settings.attributes.clear();
                    rule
                })
                .collect(),
        )
        .expect("two rules");
        let university = Rule {
            when: expr("amenity in ['university', 'college']"),
            settings: RuleSettings {
                minzoom: Some(fixed_zoom(10)),
                min_size: Some(PerFeature::Fixed(ByZoom::Steps(
                    [
                        (Zoom::new(0).unwrap(), px(80.0)),
                        (Zoom::new(13).unwrap(), px(0.0)),
                    ]
                    .into(),
                ))),
                ..RuleSettings::default()
            },
        };
        let railway = Rule {
            when: expr("railway in ['station', 'halt']"),
            settings: RuleSettings {
                minzoom: Some(fixed_zoom(12)),
                ..RuleSettings::default()
            },
        };
        let fallback = RuleSettings {
            minzoom: Some(fixed_zoom(14)),
            ..RuleSettings::default()
        };
        assert_eq!(
            rules,
            Rules {
                cases: NonEmpty::try_from_vec(vec![university, railway]).unwrap(),
                fallback: Some(fallback),
            }
        );
    }

    #[test]
    fn an_empty_rules_list_is_rejected() {
        insta::assert_snapshot!(
            rejection("poi: { rules: [] }"),
            @"error: line 1 column 15: `rules` needs at least one rule with a `where`"
        );
    }

    #[test]
    fn rules_without_a_where_are_rejected() {
        insta::assert_snapshot!(
            rejection("poi: { rules: [ { minzoom: 14 } ] }"),
            @"error: line 1 column 15: `rules` needs at least one rule with a `where`"
        );
    }

    #[test]
    fn a_catch_all_rule_must_be_last() {
        insta::assert_snapshot!(
            rejection("poi: { rules: [ { minzoom: 14 }, { where: \"amenity == 'cafe'\" } ] }"),
            @"error: line 1 column 15: a rule without `where` takes every feature, so it must be the last rule"
        );
    }

    #[test]
    fn a_rule_where_must_be_valid_cel() {
        insta::assert_snapshot!(
            rejection("poi: { rules: [ { where: \"amenity = 'cafe'\" } ] }"),
            @"error: line 1 column 26: `amenity = 'cafe'` is not a valid CEL expression: Syntax error: token recognition error at: '= '"
        );
    }

    #[test]
    fn a_rule_minzoom_cannot_exceed_its_maxzoom() {
        insta::assert_snapshot!(
            rejection("poi: { rules: [ { where: \"amenity == 'cafe'\", minzoom: 14, maxzoom: 12 } ] }"),
            @"error: line 1 column 15: minzoom 14 is above maxzoom 12"
        );
    }

    #[test]
    fn rule_attributes_must_be_a_map() {
        insta::assert_snapshot!(
            rejection("poi: { rules: [ { where: \"amenity == 'cafe'\", attributes: [name] } ] }"),
            @"error: line 1 column 59: expected mapping start"
        );
    }
}

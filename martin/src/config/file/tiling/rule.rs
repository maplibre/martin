use std::fmt;

use indexmap::IndexMap;
use serde::de::value::SeqAccessDeserializer;
use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::condition::Condition;
use super::error::TilingConfigError;
use super::primitives::{NonEmpty, debug_unrecognized};
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
    pub when: Condition,
    pub settings: RuleSettings,
}

#[derive(Clone, Default, PartialEq)]
pub struct RuleSettings {
    pub minzoom: Option<ZoomSetting>,
    pub maxzoom: Option<ZoomSetting>,
    pub simplify: Option<PixelSetting>,
    pub min_size: Option<PixelSetting>,
    pub attributes: IndexMap<String, ValueSpec>,
    pub unrecognized: UnrecognizedValues,
}

impl fmt::Debug for RuleSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("RuleSettings");
        s.field("minzoom", &self.minzoom)
            .field("maxzoom", &self.maxzoom)
            .field("simplify", &self.simplify)
            .field("min_size", &self.min_size)
            .field("attributes", &self.attributes);
        debug_unrecognized(&mut s, &self.unrecognized).finish()
    }
}

#[derive(Default, Serialize, Deserialize)]
struct RawRule {
    #[serde(default, rename = "where", skip_serializing_if = "Option::is_none")]
    when: Option<Condition>,
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
    fn split(self) -> Result<(Option<Condition>, RuleSettings), TilingConfigError> {
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

    fn join(when: Option<&Condition>, settings: &RuleSettings) -> Self {
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

    use crate::config::file::tiling::tests::{parse, rejections};

    #[test]
    fn rules_pick_the_first_match_and_may_end_in_a_catch_all() {
        let layers = parse(indoc! {"
            poi:
              rules:
                - where: { amenity: [university, college] }
                  minzoom: 10
                  min_size: { 0: 80, 13: 0 }
                - where: { railway: [station, halt] }
                  minzoom: 12
                  attributes: { class: { value: railway } }
                - minzoom: 14
        "});
        insta::assert_debug_snapshot!(layers.get("poi").unwrap().rules, @r#"
        Some(
            Rules {
                cases: [
                    Rule {
                        when: Condition {
                            amenity: OneOf(
                                [
                                    "university",
                                    "college",
                                ],
                            ),
                        },
                        settings: RuleSettings {
                            minzoom: Some(
                                z10,
                            ),
                            maxzoom: None,
                            simplify: None,
                            min_size: Some(
                                Steps(
                                    {
                                        z0: 80px,
                                        z13: 0px,
                                    },
                                ),
                            ),
                            attributes: {},
                        },
                    },
                    Rule {
                        when: Condition {
                            railway: OneOf(
                                [
                                    "station",
                                    "halt",
                                ],
                            ),
                        },
                        settings: RuleSettings {
                            minzoom: Some(
                                z12,
                            ),
                            maxzoom: None,
                            simplify: None,
                            min_size: None,
                            attributes: {
                                "class": Literal(
                                    "railway",
                                ),
                            },
                        },
                    },
                ],
                fallback: Some(
                    RuleSettings {
                        minzoom: Some(
                            z14,
                        ),
                        maxzoom: None,
                        simplify: None,
                        min_size: None,
                        attributes: {},
                    },
                ),
            },
        )
        "#);
    }

    #[test]
    fn rules_cannot_be_unreachable_or_empty() {
        insta::assert_snapshot!(rejections(&[
            "poi: { rules: [] }",
            "poi: { rules: [ { minzoom: 14 } ] }",
            "poi: { rules: [ { minzoom: 14 }, { where: { amenity: cafe } } ] }",
            "poi: { rules: [ { where: { amenity: cafe }, minzoom: 14, maxzoom: 12 } ] }",
            "poi: { rules: [ { where: { amenity: cafe }, attributes: [name] } ] }",
        ]), @"
        poi: { rules: [] }
          error: line 1 column 15: `rules` needs at least one rule with a `where`
        poi: { rules: [ { minzoom: 14 } ] }
          error: line 1 column 15: `rules` needs at least one rule with a `where`
        poi: { rules: [ { minzoom: 14 }, { where: { amenity: cafe } } ] }
          error: line 1 column 15: a rule without `where` takes every feature, so it must be the last rule
        poi: { rules: [ { where: { amenity: cafe }, minzoom: 14, maxzoom: 12 } ] }
          error: line 1 column 15: minzoom 14 is above maxzoom 12
        poi: { rules: [ { where: { amenity: cafe }, attributes: [name] } ] }
          error: line 1 column 57: expected mapping start
        ");
    }
}

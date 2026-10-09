use std::fmt;

use indexmap::IndexMap;
use serde::de::value::{MapAccessDeserializer, SeqAccessDeserializer};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::SerializeMap as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::error::TilingConfigError;
use super::primitives::{Expr, Finite, Literal, NonEmpty, forward_scalars, single_entry_map};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Condition {
    pub properties: IndexMap<String, PropertyTest>,
    pub has: Option<NonEmpty<String>>,
    pub missing: Option<NonEmpty<String>>,
    pub any: Option<NonEmpty<Self>>,
    pub all: Option<NonEmpty<Self>>,
    pub not: Option<Box<Self>>,
    pub expr: Option<Expr>,
    pub geometry: Option<NonEmpty<GeometryType>>,
    pub source_layer: Option<NameMatch>,
}

const RESERVED: [&str; 9] = [
    "has",
    "missing",
    "any",
    "all",
    "not",
    "expr",
    "geometry",
    "source_layer",
    "props",
];

impl Condition {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    fn add_property<E: de::Error>(&mut self, name: String, test: PropertyTest) -> Result<(), E> {
        if self.properties.contains_key(&name) {
            return Err(E::custom(TilingConfigError::PropertyTestedTwice(name)));
        }
        self.properties.insert(name, test);
        Ok(())
    }
}

impl Serialize for Condition {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (reserved, plain): (IndexMap<_, _>, IndexMap<_, _>) = self
            .properties
            .iter()
            .partition(|(name, _)| RESERVED.contains(&name.as_str()));
        let mut map = serializer.serialize_map(None)?;
        for (name, test) in plain {
            map.serialize_entry(name, test)?;
        }
        if !reserved.is_empty() {
            map.serialize_entry("props", &reserved)?;
        }
        if let Some(v) = &self.has {
            map.serialize_entry("has", v)?;
        }
        if let Some(v) = &self.missing {
            map.serialize_entry("missing", v)?;
        }
        if let Some(v) = &self.any {
            map.serialize_entry("any", v)?;
        }
        if let Some(v) = &self.all {
            map.serialize_entry("all", v)?;
        }
        if let Some(v) = &self.not {
            map.serialize_entry("not", v)?;
        }
        if let Some(v) = &self.expr {
            map.serialize_entry("expr", v)?;
        }
        if let Some(v) = &self.geometry {
            map.serialize_entry("geometry", v)?;
        }
        if let Some(v) = &self.source_layer {
            map.serialize_entry("source_layer", v)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Condition {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ConditionVisitor;

        impl<'de> Visitor<'de> for ConditionVisitor {
            type Value = Condition;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a map of conditions")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Condition, A::Error> {
                let mut c = Condition::default();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "has" => c.has = Some(map.next_value()?),
                        "missing" => c.missing = Some(map.next_value()?),
                        "any" => c.any = Some(map.next_value()?),
                        "all" => c.all = Some(map.next_value()?),
                        "not" => c.not = Some(map.next_value()?),
                        "expr" => c.expr = Some(map.next_value()?),
                        "geometry" => c.geometry = Some(map.next_value()?),
                        "source_layer" => c.source_layer = Some(map.next_value()?),
                        "props" => {
                            let props: IndexMap<String, PropertyTest> = map.next_value()?;
                            for (name, test) in props {
                                c.add_property(name, test)?;
                            }
                        }
                        _ => c.add_property(key, map.next_value()?)?,
                    }
                }
                if c.is_empty() {
                    return Err(de::Error::invalid_length(0, &"at least one condition"));
                }
                Ok(c)
            }
        }

        deserializer.deserialize_map(ConditionVisitor)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeometryType {
    Point,
    Line,
    Polygon,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PropertyTest {
    OneOf(NonEmpty<Literal>),
    Like(String),
    Range(Range),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Range {
    From(Bound),
    To(Bound),
    Between(Bound, Bound),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bound {
    pub value: Finite,
    pub inclusive: bool,
}

impl Range {
    fn new(from: Option<Bound>, to: Option<Bound>) -> Result<Self, TilingConfigError> {
        match (from, to) {
            (Some(from), None) => Ok(Self::From(from)),
            (None, Some(to)) => Ok(Self::To(to)),
            (Some(from), Some(to))
                if from.value < to.value
                    || (from.value == to.value && from.inclusive && to.inclusive) =>
            {
                Ok(Self::Between(from, to))
            }
            (Some(_), Some(_)) => Err(TilingConfigError::EmptyRange),
            (None, None) => Err(TilingConfigError::NoPropertyTest),
        }
    }

    fn from(self) -> Option<Bound> {
        match self {
            Self::From(b) | Self::Between(b, _) => Some(b),
            Self::To(_) => None,
        }
    }

    fn to(self) -> Option<Bound> {
        match self {
            Self::To(b) | Self::Between(_, b) => Some(b),
            Self::From(_) => None,
        }
    }
}

impl Serialize for PropertyTest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::OneOf(values) => values.serialize(serializer),
            Self::Like(pattern) => single_entry_map(serializer, "like", pattern),
            Self::Range(range) => {
                let mut map = serializer.serialize_map(None)?;
                if let Some(b) = range.from() {
                    map.serialize_entry(if b.inclusive { "gte" } else { "gt" }, &b.value)?;
                }
                if let Some(b) = range.to() {
                    map.serialize_entry(if b.inclusive { "lte" } else { "lt" }, &b.value)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for PropertyTest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PropertyTestVisitor;

        impl<'de> Visitor<'de> for PropertyTestVisitor {
            type Value = PropertyTest;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    "a value, a list of values, or a map with `like`, `gt`, `gte`, `lt` or `lte`",
                )
            }

            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<PropertyTest, A::Error> {
                NonEmpty::deserialize(SeqAccessDeserializer::new(seq)).map(PropertyTest::OneOf)
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<PropertyTest, A::Error> {
                const KEYS: &[&str] = &["like", "gt", "gte", "lt", "lte"];
                let mut like = None;
                let (mut from, mut to): (Option<Bound>, Option<Bound>) = (None, None);
                while let Some(key) = map.next_key::<String>()? {
                    let (slot, inclusive) = match key.as_str() {
                        "like" => {
                            like = Some(map.next_value::<String>()?);
                            continue;
                        }
                        "gt" => (&mut from, false),
                        "gte" => (&mut from, true),
                        "lt" => (&mut to, false),
                        "lte" => (&mut to, true),
                        _ => return Err(de::Error::unknown_field(&key, KEYS)),
                    };
                    if slot.is_some() {
                        return Err(de::Error::custom(TilingConfigError::RangeEndTwice(key)));
                    }
                    *slot = Some(Bound {
                        value: map.next_value()?,
                        inclusive,
                    });
                }
                match (like, from.is_some() || to.is_some()) {
                    (Some(pattern), false) => Ok(PropertyTest::Like(pattern)),
                    (Some(_), true) => Err(de::Error::custom(TilingConfigError::LikeWithRange)),
                    (None, _) => Range::new(from, to)
                        .map(PropertyTest::Range)
                        .map_err(de::Error::custom),
                }
            }

            forward_scalars!(Literal => |l| PropertyTest::OneOf(NonEmpty::new(l)); visit_str: &str, visit_bool: bool, visit_i64: i64, visit_u64: u64, visit_f64: f64);
        }

        deserializer.deserialize_any(PropertyTestVisitor)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum NameMatch {
    OneOf(NonEmpty<String>),
    Like(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LikeOnly {
    like: String,
}

impl Serialize for NameMatch {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::OneOf(names) => names.serialize(serializer),
            Self::Like(pattern) => single_entry_map(serializer, "like", pattern),
        }
    }
}

impl<'de> Deserialize<'de> for NameMatch {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct NameMatchVisitor;

        impl<'de> Visitor<'de> for NameMatchVisitor {
            type Value = NameMatch;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a name, a list of names, or `{ like: pattern }`")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<NameMatch, E> {
                Ok(NameMatch::OneOf(NonEmpty::new(v.to_owned())))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<NameMatch, A::Error> {
                NonEmpty::deserialize(SeqAccessDeserializer::new(seq)).map(NameMatch::OneOf)
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<NameMatch, A::Error> {
                LikeOnly::deserialize(MapAccessDeserializer::new(map))
                    .map(|l| NameMatch::Like(l.like))
            }
        }

        deserializer.deserialize_any(NameMatchVisitor)
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use indexmap::IndexMap;

    use super::{Bound, Condition, GeometryType, NameMatch, PropertyTest, Range};
    use crate::config::file::tiling::tests::{parse, rejection};
    use crate::config::file::tiling::{Expr, Finite, Literal, NonEmpty};

    #[test]
    fn a_condition_ands_every_key_of_its_map() {
        let layers = parse(indoc! {r#"
            roads:
              where:
                highway: [motorway, trunk]
                bridge: "yes"
                layer: 1
                name: { like: "A%" }
                population: { gte: 100000, lt: 1000000 }
                has: [ref]
                missing: tunnel
                not: { access: [private, no] }
                any: [ { bridge: "yes" }, { expr: "props.layer.int() > 0" } ]
                geometry: line
                source_layer: { like: "ne_%_ocean" }
                props: { any: reserved-word }
        "#});
        let condition = layers
            .get("roads")
            .and_then(|layer| layer.r#where.as_ref())
            .expect("roads has a condition");
        let strings = |items: &[&str]| {
            NonEmpty::try_from_vec(items.iter().map(|s| (*s).to_owned()).collect())
                .expect("not empty")
        };
        let literals = |items: Vec<Literal>| NonEmpty::try_from_vec(items).expect("not empty");
        let bound = |value, inclusive| Bound {
            value: Finite::new(value).expect("finite"),
            inclusive,
        };
        let one_of_string =
            |s: &str| PropertyTest::OneOf(literals(vec![Literal::String(s.to_owned())]));

        assert_eq!(
            condition.properties.keys().collect::<Vec<_>>(),
            ["highway", "bridge", "layer", "name", "population", "any"]
        );
        assert_eq!(
            condition.properties["highway"],
            PropertyTest::OneOf(literals(vec![
                Literal::String("motorway".to_owned()),
                Literal::String("trunk".to_owned()),
            ]))
        );
        assert_eq!(condition.properties["bridge"], one_of_string("yes"));
        assert_eq!(
            condition.properties["layer"],
            PropertyTest::OneOf(literals(vec![Literal::Int(1)]))
        );
        assert_eq!(
            condition.properties["name"],
            PropertyTest::Like("A%".to_owned())
        );
        assert_eq!(
            condition.properties["population"],
            PropertyTest::Range(Range::Between(
                bound(100_000.0, true),
                bound(1_000_000.0, false)
            ))
        );
        assert_eq!(condition.properties["any"], one_of_string("reserved-word"));
        assert_eq!(condition.has, Some(strings(&["ref"])));
        assert_eq!(condition.missing, Some(strings(&["tunnel"])));
        assert_eq!(
            condition.any,
            Some(
                NonEmpty::try_from_vec(vec![
                    Condition {
                        properties: IndexMap::from([("bridge".to_owned(), one_of_string("yes"))]),
                        ..Condition::default()
                    },
                    Condition {
                        expr: Expr::new("props.layer.int() > 0"),
                        ..Condition::default()
                    },
                ])
                .expect("not empty")
            )
        );
        assert_eq!(
            condition.not,
            Some(Box::new(Condition {
                properties: IndexMap::from([(
                    "access".to_owned(),
                    PropertyTest::OneOf(literals(vec![
                        Literal::String("private".to_owned()),
                        Literal::Bool(false),
                    ]))
                )]),
                ..Condition::default()
            }))
        );
        assert_eq!(condition.all, None);
        assert_eq!(condition.expr, None);
        assert_eq!(condition.geometry, Some(NonEmpty::new(GeometryType::Line)));
        assert_eq!(
            condition.source_layer,
            Some(NameMatch::Like("ne_%_ocean".to_owned()))
        );
    }

    #[test]
    fn a_condition_cannot_be_empty() {
        insta::assert_snapshot!(rejection("roads: { where: {} }"), @"error: line 1 column 17: invalid length 0, expected at least one condition");
    }

    #[test]
    fn an_any_list_cannot_be_empty() {
        insta::assert_snapshot!(rejection("roads: { where: { any: [] } }"), @"error: line 1 column 19: invalid length 0, expected at least one item");
    }

    #[test]
    fn a_has_list_cannot_be_empty() {
        insta::assert_snapshot!(rejection("roads: { where: { has: [] } }"), @"error: line 1 column 19: invalid length 0, expected at least one item");
    }

    #[test]
    fn a_property_cannot_be_tested_against_an_empty_list() {
        insta::assert_snapshot!(rejection("roads: { where: { highway: [] } }"), @"error: line 1 column 19: invalid length 0, expected at least one item");
    }

    #[test]
    fn a_property_cannot_be_tested_twice() {
        insta::assert_snapshot!(rejection("roads: { where: { highway: motorway, props: { highway: trunk } } }"), @"error: line 1 column 17: property `highway` is tested twice");
    }

    #[test]
    fn an_empty_property_test_is_rejected() {
        insta::assert_snapshot!(rejection("roads: { where: { population: {} } }"), @"error: line 1 column 31: needs one of `like`, `gt`, `gte`, `lt` or `lte`");
    }

    #[test]
    fn a_range_cannot_repeat_a_lower_bound() {
        insta::assert_snapshot!(rejection("roads: { where: { population: { gt: 1, gte: 2 } } }"), @"error: line 1 column 31: `gte` repeats an end of the range that is already set");
    }

    #[test]
    fn like_cannot_be_combined_with_a_range() {
        insta::assert_snapshot!(rejection("roads: { where: { population: { like: 'A%', gte: 2 } } }"), @"error: line 1 column 31: `like` cannot be combined with a range");
    }

    #[test]
    fn a_range_whose_ends_are_reversed_is_rejected() {
        insta::assert_snapshot!(rejection("roads: { where: { population: { gte: 5, lt: 1 } } }"), @"error: line 1 column 31: the range is empty: no number lies between its ends");
    }

    #[test]
    fn a_range_with_an_exclusive_end_at_its_other_end_is_rejected() {
        insta::assert_snapshot!(rejection("roads: { where: { population: { gt: 1, lte: 1 } } }"), @"error: line 1 column 31: the range is empty: no number lies between its ends");
    }

    #[test]
    fn a_range_bound_cannot_be_nan() {
        insta::assert_snapshot!(rejection("roads: { where: { population: { gte: .nan } } }"), @"error: line 1 column 33: invalid value: floating point `NaN`, expected a finite number");
    }

    #[test]
    fn a_range_cannot_use_between() {
        insta::assert_snapshot!(rejection("roads: { where: { population: { between: [1, 2] } } }"), @"error: line 1 column 33: unknown field `between`, expected one of like, gt, gte, lt, lte");
    }

    #[test]
    fn an_empty_expression_is_rejected() {
        insta::assert_snapshot!(rejection("roads: { where: { expr: '' } }"), @r#"error: line 1 column 19: invalid value: string "", expected an expression"#);
    }

    #[test]
    fn a_condition_cannot_match_the_label_point_geometry() {
        insta::assert_snapshot!(rejection("roads: { where: { geometry: label_point } }"), @"error: line 1 column 19: unknown variant `label_point`, expected one of point, line, polygon");
    }
}

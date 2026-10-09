use std::fmt;

use indexmap::IndexMap;
use serde::de::value::{MapAccessDeserializer, SeqAccessDeserializer};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::SerializeMap as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::error::ValueError;
use super::primitives::{Expr, Literal, NonEmpty, forward_scalars, single_entry_map};
use super::zoom::{Zoom, ZoomRange};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Literal(Literal),
    Expr(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValueSpec {
    pub value: Value,
    pub zooms: ZoomRange,
}

impl ValueSpec {
    fn plain(value: Value) -> Self {
        Self {
            value,
            zooms: ZoomRange::default(),
        }
    }
}

#[serde_with::skip_serializing_none]
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawValue {
    value: Option<Literal>,
    expr: Option<Expr>,
    minzoom: Option<Zoom>,
    maxzoom: Option<Zoom>,
}

impl TryFrom<RawValue> for ValueSpec {
    type Error = ValueError;

    fn try_from(raw: RawValue) -> Result<Self, ValueError> {
        let value = match (raw.value, raw.expr) {
            (Some(literal), None) => Value::Literal(literal),
            (None, Some(expr)) => Value::Expr(expr),
            (None, None) => return Err(ValueError::NoValue),
            (Some(_), Some(_)) => return Err(ValueError::SeveralValues),
        };
        Ok(Self {
            value,
            zooms: ZoomRange::new(raw.minzoom, raw.maxzoom)?,
        })
    }
}

impl From<&ValueSpec> for RawValue {
    fn from(spec: &ValueSpec) -> Self {
        let (value, expr) = match &spec.value {
            Value::Literal(literal) => (Some(literal.clone()), None),
            Value::Expr(expr) => (None, Some(expr.clone())),
        };
        Self {
            value,
            expr,
            minzoom: spec.zooms.minzoom(),
            maxzoom: spec.zooms.maxzoom(),
        }
    }
}

impl<'de> Deserialize<'de> for ValueSpec {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ValueSpecVisitor;

        impl<'de> Visitor<'de> for ValueSpecVisitor {
            type Value = ValueSpec;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    "an expression, a number, a boolean or a map such as `{ value: literal }`",
                )
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<ValueSpec, E> {
                Expr::new(v)
                    .map(|expr| ValueSpec::plain(Value::Expr(expr)))
                    .map_err(E::custom)
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<ValueSpec, A::Error> {
                RawValue::deserialize(MapAccessDeserializer::new(map))?
                    .try_into()
                    .map_err(de::Error::custom)
            }

            forward_scalars!(Literal => |l| ValueSpec::plain(Value::Literal(l)); visit_bool: bool, visit_i64: i64, visit_u64: u64, visit_f64: f64);
        }

        deserializer.deserialize_any(ValueSpecVisitor)
    }
}

impl Serialize for ValueSpec {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.zooms.is_unbounded() {
            match &self.value {
                Value::Expr(expr) => return expr.serialize(serializer),
                Value::Literal(literal) if !matches!(literal, Literal::String(_)) => {
                    return literal.serialize(serializer);
                }
                Value::Literal(_) => {}
            }
        }
        RawValue::from(self).serialize(serializer)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Attributes {
    #[default]
    AllProperties,
    None,
    Properties(NonEmpty<PropertySelector>),
    Columns(Columns),
}

impl Attributes {
    #[must_use]
    pub fn is_all_properties(&self) -> bool {
        *self == Self::AllProperties
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropertySelector {
    Named(String),
    Prefixed(String),
}

impl PropertySelector {
    fn parse(selector: &str) -> Result<Self, ValueError> {
        match selector.strip_suffix('*') {
            Some("") => Err(ValueError::WildcardWithoutPrefix),
            Some(prefix) => Ok(Self::Prefixed(prefix.to_owned())),
            None if selector.is_empty() => Err(ValueError::EmptyPropertyName),
            None => Ok(Self::Named(selector.to_owned())),
        }
    }
}

impl fmt::Display for PropertySelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named(name) => f.write_str(name),
            Self::Prefixed(prefix) => write!(f, "{prefix}*"),
        }
    }
}

impl Serialize for PropertySelector {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PropertySelector {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let selector = String::deserialize(deserializer)?;
        Self::parse(&selector).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Columns {
    pub computed: IndexMap<String, ValueSpec>,
    pub copied_prefixes: Vec<String>,
}

impl Serialize for Attributes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::AllProperties => serializer.serialize_none(),
            Self::None => serializer.serialize_map(Some(0))?.end(),
            Self::Properties(selectors) => selectors.as_slice().serialize(serializer),
            Self::Columns(columns) => {
                let mut map = serializer.serialize_map(None)?;
                for (name, spec) in &columns.computed {
                    map.serialize_entry(name, spec)?;
                }
                for prefix in &columns.copied_prefixes {
                    let wildcard = format!("{prefix}*");
                    map.serialize_entry(&wildcard, &wildcard)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Attributes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct AttributesVisitor;

        impl<'de> Visitor<'de> for AttributesVisitor {
            type Value = Attributes;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a list of properties or a map of output columns")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Attributes, A::Error> {
                let selectors =
                    Vec::<PropertySelector>::deserialize(SeqAccessDeserializer::new(seq))?;
                Ok(NonEmpty::try_from_vec(selectors)
                    .map_or(Attributes::None, Attributes::Properties))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Attributes, A::Error> {
                let mut computed = IndexMap::new();
                let mut copied_prefixes = Vec::new();
                while let Some(column) = map.next_key::<String>()? {
                    if let Some(prefix) = column.strip_suffix('*') {
                        let source: String = map.next_value()?;
                        if source != column || prefix.is_empty() {
                            return Err(de::Error::custom(ValueError::RenamedWildcard(column)));
                        }
                        copied_prefixes.push(prefix.to_owned());
                    } else {
                        computed.insert(column, map.next_value()?);
                    }
                }
                if computed.is_empty() && copied_prefixes.is_empty() {
                    return Ok(Attributes::None);
                }
                Ok(Attributes::Columns(Columns {
                    computed,
                    copied_prefixes,
                }))
            }
        }

        deserializer.deserialize_any(AttributesVisitor)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SortKey {
    pub expr: Expr,
    pub descending: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSortKey {
    expr: Expr,
    #[serde(default)]
    desc: bool,
}

impl<'de> Deserialize<'de> for SortKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SortKeyVisitor;

        impl<'de> Visitor<'de> for SortKeyVisitor {
            type Value = SortKey;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an expression or a map such as `{ expr: …, desc: true }`")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<SortKey, E> {
                Ok(SortKey {
                    expr: Expr::new(v).map_err(E::custom)?,
                    descending: false,
                })
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<SortKey, A::Error> {
                let raw = RawSortKey::deserialize(MapAccessDeserializer::new(map))?;
                Ok(SortKey {
                    expr: raw.expr,
                    descending: raw.desc,
                })
            }
        }

        deserializer.deserialize_any(SortKeyVisitor)
    }
}

impl Serialize for SortKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.descending {
            RawSortKey {
                expr: self.expr.clone(),
                desc: true,
            }
            .serialize(serializer)
        } else {
            self.expr.serialize(serializer)
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum IdPolicy {
    #[default]
    Keep,
    Drop,
    Expr(Expr),
}

impl IdPolicy {
    #[must_use]
    pub fn is_keep(&self) -> bool {
        *self == Self::Keep
    }
}

impl Serialize for IdPolicy {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Keep => serializer.serialize_str("keep"),
            Self::Drop => serializer.serialize_str("drop"),
            Self::Expr(expr) => single_entry_map(serializer, "expr", expr),
        }
    }
}

impl<'de> Deserialize<'de> for IdPolicy {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Word {
            Keep,
            Drop,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ExprOnly {
            expr: Expr,
        }

        struct IdVisitor;

        impl<'de> Visitor<'de> for IdVisitor {
            type Value = IdPolicy;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("`keep`, `drop` or `{ expr: … }`")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<IdPolicy, E> {
                match Word::deserialize(de::IntoDeserializer::<E>::into_deserializer(v))? {
                    Word::Keep => Ok(IdPolicy::Keep),
                    Word::Drop => Ok(IdPolicy::Drop),
                }
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<IdPolicy, A::Error> {
                ExprOnly::deserialize(MapAccessDeserializer::new(map))
                    .map(|e| IdPolicy::Expr(e.expr))
            }
        }

        deserializer.deserialize_any(IdVisitor)
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use indoc::indoc;

    use super::{Attributes, Columns, PropertySelector, Value, ValueSpec};
    use crate::config::file::tiling::tests::{parse, rejection};
    use crate::config::file::tiling::{Expr, Literal, NonEmpty, Zoom, ZoomRange};

    fn expr(source: &str) -> Value {
        Value::Expr(Expr::new(source).expect("valid CEL"))
    }

    fn zoom(zoom: u8) -> Zoom {
        Zoom::new(zoom).expect("valid zoom")
    }

    #[test]
    fn omitted_attributes_pass_every_property_through() {
        let layers = parse("roads: {}");
        assert_eq!(
            layers.get("roads").expect("layer exists").attributes,
            Attributes::AllProperties
        );
    }

    #[test]
    fn an_empty_list_or_map_emits_no_attributes() {
        let layers = parse(indoc! {"
            listed: { attributes: [] }
            mapped: { attributes: {} }
        "});
        assert_eq!(
            layers.get("listed").expect("layer exists").attributes,
            Attributes::None
        );
        assert_eq!(
            layers.get("mapped").expect("layer exists").attributes,
            Attributes::None
        );
    }

    #[test]
    fn a_list_of_attributes_passes_those_properties_through() {
        let layers = parse(r#"water_name: { attributes: [name, "name:*"] }"#);
        assert_eq!(
            layers.get("water_name").expect("layer exists").attributes,
            Attributes::Properties(
                NonEmpty::try_from_vec(vec![
                    PropertySelector::Named("name".to_owned()),
                    PropertySelector::Prefixed("name:".to_owned()),
                ])
                .expect("two selectors")
            )
        );
    }

    #[test]
    fn an_attribute_is_an_expression_or_a_literal() {
        let layers = parse(indoc! {r#"
            poi:
              attributes:
                name: name
                name_en: "feature['name:en'] != null ? feature['name:en'] : name"
                rank: "int(population) > 1000000 ? 1 : 2"
                kind: { value: road }
                layer: 0
                oneway: true
                "name:*": "name:*"
        "#});
        let expected = Attributes::Columns(Columns {
            computed: IndexMap::from([
                ("name".to_owned(), ValueSpec::plain(expr("name"))),
                (
                    "name_en".to_owned(),
                    ValueSpec::plain(expr("feature['name:en'] != null ? feature['name:en'] : name")),
                ),
                (
                    "rank".to_owned(),
                    ValueSpec::plain(expr("int(population) > 1000000 ? 1 : 2")),
                ),
                (
                    "kind".to_owned(),
                    ValueSpec::plain(Value::Literal(Literal::String("road".to_owned()))),
                ),
                (
                    "layer".to_owned(),
                    ValueSpec::plain(Value::Literal(Literal::Int(0))),
                ),
                (
                    "oneway".to_owned(),
                    ValueSpec::plain(Value::Literal(Literal::Bool(true))),
                ),
            ]),
            copied_prefixes: vec!["name:".to_owned()],
        });
        assert_eq!(
            layers.get("poi").expect("layer exists").attributes,
            expected
        );
    }

    #[test]
    fn an_attribute_can_be_limited_to_zooms() {
        let layers = parse(indoc! {r#"
            transportation:
              attributes:
                layer: { expr: "int(layer)", minzoom: 9 }
                tunnel: { value: true, minzoom: 11, maxzoom: 14 }
        "#});
        let expected = Attributes::Columns(Columns {
            computed: IndexMap::from([
                (
                    "layer".to_owned(),
                    ValueSpec {
                        value: expr("int(layer)"),
                        zooms: ZoomRange::new(Some(zoom(9)), None).expect("valid range"),
                    },
                ),
                (
                    "tunnel".to_owned(),
                    ValueSpec {
                        value: Value::Literal(Literal::Bool(true)),
                        zooms: ZoomRange::new(Some(zoom(11)), Some(zoom(14))).expect("valid range"),
                    },
                ),
            ]),
            copied_prefixes: vec![],
        });
        assert_eq!(
            layers
                .get("transportation")
                .expect("layer exists")
                .attributes,
            expected
        );
    }

    #[test]
    fn an_attribute_must_be_valid_cel() {
        insta::assert_snapshot!(
            rejection("poi: { attributes: { name: 'name ==' } }"),
            @"error: line 1 column 28: `name ==` is not a valid CEL expression: Syntax error: mismatched input '<EOF>' expecting {'[', '{', '(', '.', '-', '!', 'true', 'false', 'null', NUM_FLOAT, NUM_INT, NUM_UINT, STRING, BYTES, IDENTIFIER}"
        );
    }

    #[test]
    fn an_attribute_cannot_have_both_value_and_expr() {
        insta::assert_snapshot!(
            rejection("poi: { attributes: { name: { expr: name, value: x } } }"),
            @"error: line 1 column 28: pick one of `value` or `expr`"
        );
    }

    #[test]
    fn an_attribute_needs_a_value_or_an_expr() {
        insta::assert_snapshot!(
            rejection("poi: { attributes: { name: { minzoom: 9 } } }"),
            @"error: line 1 column 28: needs one of `value` or `expr`"
        );
    }

    #[test]
    fn an_attribute_minzoom_beyond_the_maximum_is_rejected() {
        insta::assert_snapshot!(
            rejection("poi: { attributes: { name: { expr: name, minzoom: 31 } } }"),
            @"error: line 1 column 42: invalid value: integer `31`, expected a zoom from 0 to 30"
        );
    }

    #[test]
    fn an_attribute_minzoom_above_its_maxzoom_is_rejected() {
        insta::assert_snapshot!(
            rejection("poi: { attributes: { name: { expr: name, minzoom: 12, maxzoom: 4 } } }"),
            @"error: line 1 column 28: minzoom 12 is above maxzoom 4"
        );
    }

    #[test]
    fn an_attribute_cannot_be_a_sort_key() {
        insta::assert_snapshot!(
            rejection("poi: { attributes: { name: { expr: name, desc: true } } }"),
            @"error: line 1 column 42: unknown field `desc`, expected one of value, expr, minzoom, maxzoom"
        );
    }

    #[test]
    fn a_wildcard_column_must_copy_its_own_properties() {
        insta::assert_snapshot!(
            rejection(r#"poi: { attributes: { "name:*": name } }"#),
            @r#"error: line 1 column 20: a wildcard column copies the properties it names: write `"name:*": "name:*"`"#
        );
    }

    #[test]
    fn a_single_column_cannot_copy_a_wildcard() {
        insta::assert_snapshot!(
            rejection(r#"poi: { attributes: { name: "name:*" } }"#),
            @"error: line 1 column 28: `name:*` is not a valid CEL expression: Syntax error: mismatched input ':' expecting {<EOF>, '==', '!=', 'in', '<', '<=', '>=', '>', '&&', '||', '[', '.', '-', '?', '+', '*', '/', '%'}"
        );
    }

    #[test]
    fn a_bare_wildcard_selector_is_rejected() {
        insta::assert_snapshot!(
            rejection(r#"poi: { attributes: ["*"] }"#),
            @"error: line 1 column 21: `*` needs a prefix before it, such as `name:*`"
        );
    }
}

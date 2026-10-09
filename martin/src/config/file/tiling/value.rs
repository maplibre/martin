use std::fmt;

use indexmap::IndexMap;
use serde::de::value::{MapAccessDeserializer, SeqAccessDeserializer};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::SerializeMap as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::condition::Condition;
use super::error::TilingConfigError;
use super::primitives::{Expr, Literal, NonEmpty, forward_scalars, single_entry_map};
use super::zoom::{Zoom, ZoomRange};

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Ref {
    Property(String),
    Let(String),
}

const LET_PREFIX: &str = "let.";

impl Ref {
    fn parse(reference: &str) -> Result<Self, TilingConfigError> {
        match reference.strip_prefix(LET_PREFIX) {
            Some("") => Err(TilingConfigError::EmptyLetName),
            Some(name) => Ok(Self::Let(name.to_owned())),
            None if reference.is_empty() => Err(TilingConfigError::EmptyPropertyName),
            None => Ok(Self::Property(reference.to_owned())),
        }
    }
}

impl fmt::Debug for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Property(name) => write!(f, "{name:?}"),
            Self::Let(name) => write!(f, "let.{name}"),
        }
    }
}

impl Serialize for Ref {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Property(name) => serializer.serialize_str(name),
            Self::Let(name) => serializer.collect_str(&format_args!("{LET_PREFIX}{name}")),
        }
    }
}

impl<'de> Deserialize<'de> for Ref {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let reference = String::deserialize(deserializer)?;
        Self::parse(&reference).map_err(de::Error::custom)
    }
}

#[derive(Clone, PartialEq)]
pub enum Value {
    Literal(Literal),
    Copy {
        from: Ref,
        otherwise: Option<Box<Self>>,
    },
    Coalesce(NonEmpty<Ref>),
    Struct(IndexMap<String, Ref>),
    Match(Match<Self>),
    Lookup(Lookup<Self>),
    Expr(Expr),
}

impl Value {
    fn copy_of(reference: &str) -> Result<Self, TilingConfigError> {
        Ref::parse(reference).map(|from| Self::Copy {
            from,
            otherwise: None,
        })
    }

    fn as_plain_copy(&self) -> Option<&Ref> {
        if let Self::Copy {
            from,
            otherwise: None,
        } = self
        {
            Some(from)
        } else {
            None
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Literal(v) => f.debug_tuple("Literal").field(v).finish(),
            Self::Copy {
                from,
                otherwise: None,
            } => f.debug_tuple("Copy").field(from).finish(),
            Self::Copy {
                from,
                otherwise: Some(otherwise),
            } => f
                .debug_struct("Copy")
                .field("from", from)
                .field("otherwise", otherwise)
                .finish(),
            Self::Coalesce(v) => f.debug_tuple("Coalesce").field(v).finish(),
            Self::Struct(v) => f.debug_tuple("Struct").field(v).finish(),
            Self::Match(v) => fmt::Debug::fmt(v, f),
            Self::Lookup(v) => fmt::Debug::fmt(v, f),
            Self::Expr(v) => fmt::Debug::fmt(v, f),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Match<T> {
    pub cases: NonEmpty<Case<T>>,
    pub otherwise: Option<Box<T>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Case<T> {
    pub when: Condition,
    pub then: T,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lookup<T> {
    pub subject: Ref,
    pub table: IndexMap<String, T>,
    pub otherwise: Option<Box<T>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cast {
    Int,
    Float,
    String,
    Bool,
}

#[derive(Clone, PartialEq)]
pub struct ValueSpec {
    pub value: Value,
    pub cast: Option<Cast>,
    pub null_if: Option<Literal>,
    pub zooms: ZoomRange,
    pub r#where: Option<Condition>,
}

impl ValueSpec {
    fn plain(value: Value) -> Self {
        Self {
            value,
            cast: None,
            null_if: None,
            zooms: ZoomRange::default(),
            r#where: None,
        }
    }

    fn is_plain(&self) -> bool {
        self.cast.is_none()
            && self.null_if.is_none()
            && self.zooms.is_unbounded()
            && self.r#where.is_none()
    }
}

impl fmt::Debug for ValueSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_plain() {
            return fmt::Debug::fmt(&self.value, f);
        }
        let mut s = f.debug_struct("ValueSpec");
        s.field("value", &self.value);
        if let Some(cast) = &self.cast {
            s.field("cast", cast);
        }
        if let Some(null_if) = &self.null_if {
            s.field("null_if", null_if);
        }
        if let Some(minzoom) = self.zooms.minzoom() {
            s.field("minzoom", &minzoom);
        }
        if let Some(maxzoom) = self.zooms.maxzoom() {
            s.field("maxzoom", &maxzoom);
        }
        if let Some(condition) = &self.r#where {
            s.field("where", condition);
        }
        s.finish()
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawValue {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<Literal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    from: Option<Ref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    coalesce: Option<NonEmpty<Ref>>,
    #[serde(default, rename = "struct", skip_serializing_if = "Option::is_none")]
    r#struct: Option<IndexMap<String, Ref>>,
    #[serde(default, rename = "match", skip_serializing_if = "Option::is_none")]
    r#match: Option<Match<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lookup: Option<Ref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    map: Option<IndexMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expr: Option<Expr>,
    #[serde(default, rename = "else", skip_serializing_if = "Option::is_none")]
    otherwise: Option<Value>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    cast: Option<Cast>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    null_if: Option<Literal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    minzoom: Option<Zoom>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    maxzoom: Option<Zoom>,
    #[serde(default, rename = "where", skip_serializing_if = "Option::is_none")]
    r#where: Option<Condition>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    desc: bool,
}

impl RawValue {
    fn into_spec(mut self) -> Result<ValueSpec, TilingConfigError> {
        let zooms = ZoomRange::new(self.minzoom.take(), self.maxzoom.take())?;
        let cast = self.cast.take();
        let null_if = self.null_if.take();
        let condition = self.r#where.take();
        let value = self.into_value()?;
        Ok(ValueSpec {
            value,
            cast,
            null_if,
            zooms,
            r#where: condition,
        })
    }

    fn into_nested_value(self) -> Result<Value, TilingConfigError> {
        let modifiers = [
            ("type", self.cast.is_some()),
            ("null_if", self.null_if.is_some()),
            ("minzoom", self.minzoom.is_some()),
            ("maxzoom", self.maxzoom.is_some()),
            ("where", self.r#where.is_some()),
        ];
        if let Some((key, _)) = modifiers.iter().find(|(_, set)| *set) {
            return Err(TilingConfigError::WholeAttributeOnly(key));
        }
        self.into_value()
    }

    fn into_value(self) -> Result<Value, TilingConfigError> {
        if self.desc {
            return Err(TilingConfigError::DescOutsideSortBy);
        }
        let Self {
            value,
            from,
            coalesce,
            r#struct,
            r#match,
            lookup,
            mut map,
            expr,
            mut otherwise,
            ..
        } = self;
        let mut kinds: Vec<(&'static str, Value)> = Vec::new();
        if let Some(v) = value {
            kinds.push(("value", Value::Literal(v)));
        }
        if let Some(from) = from {
            let otherwise = otherwise.take().map(Box::new);
            kinds.push(("from", Value::Copy { from, otherwise }));
        }
        if let Some(refs) = coalesce {
            kinds.push(("coalesce", Value::Coalesce(refs)));
        }
        if let Some(fields) = r#struct {
            if fields.is_empty() {
                return Err(TilingConfigError::EmptyStruct);
            }
            kinds.push(("struct", Value::Struct(fields)));
        }
        if let Some(m) = r#match {
            kinds.push(("match", Value::Match(m)));
        }
        if let Some(subject) = lookup {
            let table = Lookup::new(subject, map.take(), otherwise.take())?;
            kinds.push(("lookup", Value::Lookup(table)));
        }
        if let Some(expr) = expr {
            kinds.push(("expr", Value::Expr(expr)));
        }
        let (kind, value) = match kinds.len() {
            1 => kinds.remove(0),
            0 => return Err(TilingConfigError::NoValue),
            _ => {
                let keys = kinds.iter().map(|(key, _)| *key).collect();
                return Err(TilingConfigError::SeveralValues(keys));
            }
        };
        if map.is_some() {
            return Err(TilingConfigError::MapWithoutLookup);
        }
        if otherwise.is_some() {
            return Err(TilingConfigError::ElseNotApplicable(kind));
        }
        Ok(value)
    }

    fn from_value(value: &Value) -> Self {
        match value.clone() {
            Value::Literal(v) => Self {
                value: Some(v),
                ..Self::default()
            },
            Value::Copy { from, otherwise } => Self {
                from: Some(from),
                otherwise: otherwise.map(|v| *v),
                ..Self::default()
            },
            Value::Coalesce(refs) => Self {
                coalesce: Some(refs),
                ..Self::default()
            },
            Value::Struct(fields) => Self {
                r#struct: Some(fields),
                ..Self::default()
            },
            Value::Match(m) => Self {
                r#match: Some(m),
                ..Self::default()
            },
            Value::Lookup(l) => Self {
                lookup: Some(l.subject),
                map: Some(l.table),
                otherwise: l.otherwise.map(|v| *v),
                ..Self::default()
            },
            Value::Expr(expr) => Self {
                expr: Some(expr),
                ..Self::default()
            },
        }
    }

    fn from_spec(spec: &ValueSpec) -> Self {
        Self {
            cast: spec.cast,
            null_if: spec.null_if.clone(),
            minzoom: spec.zooms.minzoom(),
            maxzoom: spec.zooms.maxzoom(),
            r#where: spec.r#where.clone(),
            ..Self::from_value(&spec.value)
        }
    }
}

impl<T> Lookup<T> {
    pub fn new(
        subject: Ref,
        table: Option<IndexMap<String, T>>,
        otherwise: Option<T>,
    ) -> Result<Self, TilingConfigError> {
        match table {
            Some(table) if !table.is_empty() => Ok(Self {
                subject,
                table,
                otherwise: otherwise.map(Box::new),
            }),
            Some(_) => Err(TilingConfigError::EmptyLookupMap),
            None => Err(TilingConfigError::LookupWithoutMap),
        }
    }
}

impl<T: Serialize> Serialize for Lookup<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("lookup", &self.subject)?;
        map.serialize_entry("map", &self.table)?;
        if let Some(otherwise) = &self.otherwise {
            map.serialize_entry("else", otherwise)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct NestedValueVisitor;

        impl<'de> Visitor<'de> for NestedValueVisitor {
            type Value = Value;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a literal or a map such as `{ from: property }`")
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Value, A::Error> {
                RawValue::deserialize(MapAccessDeserializer::new(map))?
                    .into_nested_value()
                    .map_err(de::Error::custom)
            }

            forward_scalars!(Literal => Value::Literal; visit_bool: bool, visit_i64: i64, visit_u64: u64, visit_f64: f64, visit_str: &str);
        }

        deserializer.deserialize_any(NestedValueVisitor)
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Self::Literal(v) = self {
            return v.serialize(serializer);
        }
        RawValue::from_value(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ValueSpec {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ValueSpecVisitor;

        impl<'de> Visitor<'de> for ValueSpecVisitor {
            type Value = ValueSpec;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a property name or a map such as `{ value: literal }`")
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<ValueSpec, A::Error> {
                RawValue::deserialize(MapAccessDeserializer::new(map))?
                    .into_spec()
                    .map_err(de::Error::custom)
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<ValueSpec, E> {
                Value::copy_of(v).map(ValueSpec::plain).map_err(E::custom)
            }
        }

        deserializer.deserialize_any(ValueSpecVisitor)
    }
}

impl Serialize for ValueSpec {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.is_plain()
            && let Some(from) = self.value.as_plain_copy()
        {
            return from.serialize(serializer);
        }
        RawValue::from_spec(self).serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Match<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCase<T> {
            #[serde(default, rename = "if")]
            when: Option<Condition>,
            #[serde(default = "Option::default")]
            value: Option<T>,
            #[serde(default = "Option::default", rename = "else")]
            otherwise: Option<T>,
        }

        struct CasesVisitor<T>(std::marker::PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for CasesVisitor<T> {
            type Value = Match<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a list of `{ if: condition, value: … }` cases, optionally ending in `{ else: … }`")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Match<T>, A::Error> {
                let raw = Vec::<RawCase<T>>::deserialize(SeqAccessDeserializer::new(seq))?;
                let mut cases = Vec::with_capacity(raw.len());
                let mut otherwise = None;
                for case in raw {
                    if otherwise.is_some() {
                        return Err(de::Error::custom(TilingConfigError::ElseNotLast));
                    }
                    match case {
                        RawCase {
                            when: Some(when),
                            value: Some(then),
                            otherwise: None,
                        } => {
                            cases.push(Case { when, then });
                        }
                        RawCase {
                            when: None,
                            value: None,
                            otherwise: Some(v),
                        } => {
                            otherwise = Some(Box::new(v));
                        }
                        RawCase {
                            when: Some(_),
                            value: None,
                            otherwise: None,
                        } => {
                            return Err(de::Error::custom(TilingConfigError::CaseWithoutValue));
                        }
                        _ => {
                            return Err(de::Error::custom(TilingConfigError::MalformedCase));
                        }
                    }
                }
                let cases = NonEmpty::try_from_vec(cases)
                    .ok_or_else(|| de::Error::custom(TilingConfigError::NoIfCase))?;
                Ok(Match { cases, otherwise })
            }
        }

        deserializer.deserialize_seq(CasesVisitor(std::marker::PhantomData))
    }
}

impl<T: Serialize> Serialize for Match<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq as _;

        struct IfCase<'a, T>(&'a Case<T>);
        impl<T: Serialize> Serialize for IfCase<'_, T> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("if", &self.0.when)?;
                map.serialize_entry("value", &self.0.then)?;
                map.end()
            }
        }

        struct ElseCase<'a, T>(&'a T);
        impl<T: Serialize> Serialize for ElseCase<'_, T> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                single_entry_map(serializer, "else", self.0)
            }
        }

        let mut seq = serializer.serialize_seq(None)?;
        for case in &self.cases {
            seq.serialize_element(&IfCase(case))?;
        }
        if let Some(otherwise) = &self.otherwise {
            seq.serialize_element(&ElseCase(otherwise))?;
        }
        seq.end()
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

#[derive(Clone, PartialEq, Eq)]
pub enum PropertySelector {
    Named(String),
    Prefixed(String),
}

impl PropertySelector {
    fn parse(selector: &str) -> Result<Self, TilingConfigError> {
        match selector.strip_suffix('*') {
            Some("") => Err(TilingConfigError::WildcardWithoutPrefix),
            Some(prefix) => Ok(Self::Prefixed(prefix.to_owned())),
            None if selector.is_empty() => Err(TilingConfigError::EmptyPropertyName),
            None => Ok(Self::Named(selector.to_owned())),
        }
    }
}

impl fmt::Debug for PropertySelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named(name) => write!(f, "{name:?}"),
            Self::Prefixed(prefix) => write!(f, "{:?}", format!("{prefix}*")),
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
                            return Err(de::Error::custom(TilingConfigError::RenamedWildcard(
                                column,
                            )));
                        }
                        copied_prefixes.push(prefix.to_owned());
                    } else {
                        let spec: ValueSpec = map.next_value()?;
                        if let Value::Copy {
                            from: Ref::Property(p),
                            ..
                        } = &spec.value
                            && p.ends_with('*')
                        {
                            return Err(de::Error::custom(
                                TilingConfigError::WildcardIntoOneColumn {
                                    column,
                                    property: p.clone(),
                                },
                            ));
                        }
                        computed.insert(column, spec);
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

#[derive(Clone, PartialEq)]
pub struct Computed(pub Value);

impl fmt::Debug for Computed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl<'de> Deserialize<'de> for Computed {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ComputedVisitor;

        impl<'de> Visitor<'de> for ComputedVisitor {
            type Value = Computed;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a property name or a map such as `{ expr: … }`")
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Computed, A::Error> {
                RawValue::deserialize(MapAccessDeserializer::new(map))?
                    .into_nested_value()
                    .map(Computed)
                    .map_err(de::Error::custom)
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Computed, E> {
                Value::copy_of(v).map(Computed).map_err(E::custom)
            }
        }

        deserializer.deserialize_any(ComputedVisitor)
    }
}

impl Serialize for Computed {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Some(from) = self.0.as_plain_copy() {
            return from.serialize(serializer);
        }
        RawValue::from_value(&self.0).serialize(serializer)
    }
}

#[derive(Clone, PartialEq)]
pub struct SortKey {
    pub value: Computed,
    pub descending: bool,
}

impl fmt::Debug for SortKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.descending {
            f.debug_tuple("Descending").field(&self.value).finish()
        } else {
            fmt::Debug::fmt(&self.value, f)
        }
    }
}

impl<'de> Deserialize<'de> for SortKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SortKeyVisitor;

        impl<'de> Visitor<'de> for SortKeyVisitor {
            type Value = SortKey;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a property name or a map such as `{ expr: …, desc: true }`")
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<SortKey, A::Error> {
                let mut raw = RawValue::deserialize(MapAccessDeserializer::new(map))?;
                let descending = std::mem::take(&mut raw.desc);
                let value = raw.into_nested_value().map_err(de::Error::custom)?;
                Ok(SortKey {
                    value: Computed(value),
                    descending,
                })
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<SortKey, E> {
                let value = Value::copy_of(v).map(Computed).map_err(E::custom)?;
                Ok(SortKey {
                    value,
                    descending: false,
                })
            }
        }

        deserializer.deserialize_any(SortKeyVisitor)
    }
}

impl Serialize for SortKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if !self.descending {
            return self.value.serialize(serializer);
        }
        RawValue {
            desc: true,
            ..RawValue::from_value(&self.value.0)
        }
        .serialize(serializer)
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
    use indoc::indoc;

    use crate::config::file::tiling::tests::{parse, rejections};

    #[test]
    fn omitted_attributes_pass_every_property_through() {
        let layers = parse("roads: {}");
        insta::assert_debug_snapshot!(layers.get("roads").unwrap().attributes, @"AllProperties");
    }

    #[test]
    fn an_empty_list_or_map_emits_no_attributes() {
        let layers = parse(indoc! {"
            listed: { attributes: [] }
            mapped: { attributes: {} }
        "});
        insta::assert_debug_snapshot!(
            (&layers.get("listed").unwrap().attributes, &layers.get("mapped").unwrap().attributes),
            @"
        (
            None,
            None,
        )
        "
        );
    }

    #[test]
    fn a_list_of_attributes_passes_those_properties_through() {
        let layers = parse(r#"water_name: { attributes: [name, "name:*"] }"#);
        insta::assert_debug_snapshot!(layers.get("water_name").unwrap().attributes, @r#"
        Properties(
            [
                "name",
                "name:*",
            ],
        )
        "#);
    }

    #[test]
    fn an_attribute_copies_a_property_or_writes_a_literal() {
        let layers = parse(indoc! {r#"
            poi:
              attributes:
                name: name
                class: let.class
                kind: { value: road }
                name_en: { coalesce: ["name:en", name] }
                label: { struct: { default: name, en: "name:en" } }
                "name:*": "name:*"
        "#});
        insta::assert_debug_snapshot!(layers.get("poi").unwrap().attributes, @r#"
        Columns(
            Columns {
                computed: {
                    "name": Copy(
                        "name",
                    ),
                    "class": Copy(
                        let.class,
                    ),
                    "kind": Literal(
                        "road",
                    ),
                    "name_en": Coalesce(
                        [
                            "name:en",
                            "name",
                        ],
                    ),
                    "label": Struct(
                        {
                            "default": "name",
                            "en": "name:en",
                        },
                    ),
                },
                copied_prefixes: [
                    "name:",
                ],
            },
        )
        "#);
    }

    #[test]
    fn an_attribute_can_be_cast_limited_to_zooms_or_conditional() {
        let layers = parse(indoc! {r#"
            transportation:
              attributes:
                lanes: { from: lanes, type: int, else: 1 }
                layer: { from: layer, type: int, null_if: 0, minzoom: 9 }
                rank: { expr: "props.population.int() > 1000000 ? 1 : 2", type: int }
                tunnel: { value: true, where: { tunnel: "yes" }, minzoom: 11, maxzoom: 14 }
        "#});
        insta::assert_debug_snapshot!(layers.get("transportation").unwrap().attributes, @r#"
        Columns(
            Columns {
                computed: {
                    "lanes": ValueSpec {
                        value: Copy {
                            from: "lanes",
                            otherwise: Literal(
                                1,
                            ),
                        },
                        cast: Int,
                    },
                    "layer": ValueSpec {
                        value: Copy(
                            "layer",
                        ),
                        cast: Int,
                        null_if: 0,
                        minzoom: z9,
                    },
                    "rank": ValueSpec {
                        value: Expr("props.population.int() > 1000000 ? 1 : 2"),
                        cast: Int,
                    },
                    "tunnel": ValueSpec {
                        value: Literal(
                            true,
                        ),
                        minzoom: z11,
                        maxzoom: z14,
                        where: Condition {
                            tunnel: OneOf(
                                [
                                    "yes",
                                ],
                            ),
                        },
                    },
                },
                copied_prefixes: [],
            },
        )
        "#);
    }

    #[test]
    fn an_attribute_can_pick_its_value_by_case_or_by_table() {
        let layers = parse(indoc! {r#"
            poi:
              attributes:
                brunnel:
                  match:
                    - { if: { bridge: "yes" }, value: bridge }
                    - { if: { tunnel: "yes" }, value: tunnel }
                subclass:
                  match:
                    - { if: { amenity: place_of_worship }, value: { from: religion } }
                    - else: { coalesce: [amenity, shop] }
                group:
                  lookup: let.subclass
                  map: { hospital: health, clinic: health }
                  else: { expr: "let.subclass" }
        "#});
        insta::assert_debug_snapshot!(layers.get("poi").unwrap().attributes, @r#"
        Columns(
            Columns {
                computed: {
                    "brunnel": Match {
                        cases: [
                            Case {
                                when: Condition {
                                    bridge: OneOf(
                                        [
                                            "yes",
                                        ],
                                    ),
                                },
                                then: Literal(
                                    "bridge",
                                ),
                            },
                            Case {
                                when: Condition {
                                    tunnel: OneOf(
                                        [
                                            "yes",
                                        ],
                                    ),
                                },
                                then: Literal(
                                    "tunnel",
                                ),
                            },
                        ],
                        otherwise: None,
                    },
                    "subclass": Match {
                        cases: [
                            Case {
                                when: Condition {
                                    amenity: OneOf(
                                        [
                                            "place_of_worship",
                                        ],
                                    ),
                                },
                                then: Copy(
                                    "religion",
                                ),
                            },
                        ],
                        otherwise: Some(
                            Coalesce(
                                [
                                    "amenity",
                                    "shop",
                                ],
                            ),
                        ),
                    },
                    "group": Lookup {
                        subject: let.subclass,
                        table: {
                            "hospital": Literal(
                                "health",
                            ),
                            "clinic": Literal(
                                "health",
                            ),
                        },
                        otherwise: Some(
                            Expr("let.subclass"),
                        ),
                    },
                },
                copied_prefixes: [],
            },
        )
        "#);
    }

    #[test]
    fn an_attribute_cannot_be_ambiguous_or_empty() {
        insta::assert_snapshot!(rejections(&[
            "poi: { attributes: { name: { from: name, value: x } } }",
            "poi: { attributes: { name: { minzoom: 9 } } }",
            "poi: { attributes: { name: { from: '' } } }",
            "poi: { attributes: { name: { from: 'let.' } } }",
            "poi: { attributes: { name: { coalesce: [] } } }",
            "poi: { attributes: { name: { struct: {} } } }",
            "poi: { attributes: { name: { from: name, type: date } } }",
            "poi: { attributes: { name: { from: name, minzoom: 31 } } }",
            "poi: { attributes: { name: { from: name, minzoom: 12, maxzoom: 4 } } }",
            "poi: { attributes: { name: { from: name, desc: true } } }",
            "poi: { attributes: { name: { match: [] } } }",
            "poi: { attributes: { name: { match: [ { else: a } ] } } }",
            "poi: { attributes: { name: { match: [ { else: a }, { if: { x: 1 }, value: b } ] } } }",
            "poi: { attributes: { name: { match: [ { if: { x: 1 } } ] } } }",
            "poi: { attributes: { name: { match: [ { if: { x: 1 }, value: a, else: b } ] } } }",
            "poi: { attributes: { name: { match: [ { if: { x: 1 }, value: { from: a, minzoom: 3 } } ] } } }",
            "poi: { attributes: { name: { lookup: kind, map: {} } } }",
            "poi: { attributes: { name: { value: x, else: y } } }",
            r#"poi: { attributes: { "name:*": name } }"#,
            r#"poi: { attributes: { name: "name:*" } }"#,
            r#"poi: { attributes: ["*"] }"#,
        ]), @r#"
        poi: { attributes: { name: { from: name, value: x } } }
          error: line 1 column 28: pick one of `value`, `from`
        poi: { attributes: { name: { minzoom: 9 } } }
          error: line 1 column 28: needs one of `value`, `from`, `coalesce`, `struct`, `match`, `lookup` or `expr`
        poi: { attributes: { name: { from: '' } } }
          error: line 1 column 36: a property name cannot be empty
        poi: { attributes: { name: { from: 'let.' } } }
          error: line 1 column 36: `let.` needs the name of a computed value after it
        poi: { attributes: { name: { coalesce: [] } } }
          error: line 1 column 30: invalid length 0, expected at least one item
        poi: { attributes: { name: { struct: {} } } }
          error: line 1 column 28: `struct` needs at least one field
        poi: { attributes: { name: { from: name, type: date } } }
          error: line 1 column 48: unknown variant `date`, expected one of int, float, string, bool
        poi: { attributes: { name: { from: name, minzoom: 31 } } }
          error: line 1 column 42: invalid value: integer `31`, expected a zoom from 0 to 30
        poi: { attributes: { name: { from: name, minzoom: 12, maxzoom: 4 } } }
          error: line 1 column 28: minzoom 12 is above maxzoom 4
        poi: { attributes: { name: { from: name, desc: true } } }
          error: line 1 column 28: `desc` only applies to a key of `sort_by`
        poi: { attributes: { name: { match: [] } } }
          error: line 1 column 37: `match` needs at least one `if` case
        poi: { attributes: { name: { match: [ { else: a } ] } } }
          error: line 1 column 37: `match` needs at least one `if` case
        poi: { attributes: { name: { match: [ { else: a }, { if: { x: 1 }, value: b } ] } } }
          error: line 1 column 37: `else` must be the last case
        poi: { attributes: { name: { match: [ { if: { x: 1 } } ] } } }
          error: line 1 column 37: a case with `if` needs a `value`
        poi: { attributes: { name: { match: [ { if: { x: 1 }, value: a, else: b } ] } } }
          error: line 1 column 37: a case is either `{ if: condition, value: … }` or `{ else: … }`
        poi: { attributes: { name: { match: [ { if: { x: 1 }, value: { from: a, minzoom: 3 } } ] } } }
          error: line 1 column 62: `minzoom` only applies to a whole attribute
        poi: { attributes: { name: { lookup: kind, map: {} } } }
          error: line 1 column 28: `map` needs at least one entry
        poi: { attributes: { name: { value: x, else: y } } }
          error: line 1 column 28: `else` does not apply to `value`; it goes with `from` or `lookup`
        poi: { attributes: { "name:*": name } }
          error: line 1 column 20: a wildcard column copies the properties it names: write `"name:*": "name:*"`
        poi: { attributes: { name: "name:*" } }
          error: line 1 column 20: `name` is one column, so it cannot copy every `name:*` property
        poi: { attributes: ["*"] }
          error: line 1 column 21: `*` needs a prefix before it, such as `name:*`
        "#);
    }
}

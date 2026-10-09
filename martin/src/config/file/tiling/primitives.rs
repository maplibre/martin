use std::fmt;
use std::marker::PhantomData;

use serde::de::value::{MapAccessDeserializer, SeqAccessDeserializer};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::SerializeMap as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::error::InvalidExpr;

macro_rules! forward_scalars {
    ($target:ty => $wrap:expr; $($visit:ident: $scalar:ty),+ $(,)?) => {
        $(
            fn $visit<E: serde::de::Error>(self, v: $scalar) -> Result<Self::Value, E> {
                <$target as serde::Deserialize>::deserialize(
                    serde::de::IntoDeserializer::<E>::into_deserializer(v),
                )
                .map($wrap)
            }
        )+
    };
}
pub(super) use forward_scalars;

#[derive(Clone, Debug, PartialEq)]
pub struct NonEmpty<T>(Vec<T>);

impl<T> NonEmpty<T> {
    pub fn new(first: T) -> Self {
        Self(vec![first])
    }

    #[must_use]
    pub fn try_from_vec(items: Vec<T>) -> Option<Self> {
        (!items.is_empty()).then_some(Self(items))
    }

    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
    }

    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }
}

impl<'a, T> IntoIterator for &'a NonEmpty<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<T: Serialize> Serialize for NonEmpty<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0.as_slice() {
            [single] => single.serialize(serializer),
            items => items.serialize(serializer),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for NonEmpty<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OneOrMore<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for OneOrMore<T> {
            type Value = NonEmpty<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("one item or a list of at least one item")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
                let items = Vec::<T>::deserialize(SeqAccessDeserializer::new(seq))?;
                NonEmpty::try_from_vec(items)
                    .ok_or_else(|| de::Error::invalid_length(0, &"at least one item"))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                T::deserialize(MapAccessDeserializer::new(map)).map(NonEmpty::new)
            }

            forward_scalars!(T => NonEmpty::new; visit_str: &str, visit_bool: bool, visit_i64: i64, visit_u64: u64, visit_f64: f64);
        }

        deserializer.deserialize_any(OneOrMore(PhantomData))
    }
}

pub fn single_entry_map<S: Serializer>(
    serializer: S,
    key: &str,
    value: &impl Serialize,
) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(1))?;
    map.serialize_entry(key, value)?;
    map.end()
}

pub fn checked_map<'de, D, R, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    R: Deserialize<'de>,
    T: TryFrom<R>,
    T::Error: fmt::Display,
{
    checked_map_with(deserializer, T::try_from)
}

pub fn checked_map_with<'de, D, R, T, E>(
    deserializer: D,
    check: impl FnOnce(R) -> Result<T, E>,
) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    R: Deserialize<'de>,
    E: fmt::Display,
{
    struct CheckedVisitor<R, F>(F, PhantomData<R>);

    impl<'de, R, T, E, F> Visitor<'de> for CheckedVisitor<R, F>
    where
        R: Deserialize<'de>,
        E: fmt::Display,
        F: FnOnce(R) -> Result<T, E>,
    {
        type Value = T;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a map")
        }

        fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<T, A::Error> {
            let raw = R::deserialize(MapAccessDeserializer::new(map))?;
            (self.0)(raw).map_err(de::Error::custom)
        }
    }

    deserializer.deserialize_map(CheckedVisitor(check, PhantomData))
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize)]
pub struct Finite(f64);

impl Finite {
    #[must_use]
    pub fn new(value: f64) -> Option<Self> {
        value.is_finite().then_some(Self(value))
    }

    #[must_use]
    pub fn get(self) -> f64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for Finite {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f64::deserialize(deserializer)?;
        Self::new(value).ok_or_else(|| {
            de::Error::invalid_value(de::Unexpected::Float(value), &"a finite number")
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    Bool(bool),
    Int(i64),
    Float(Finite),
    String(String),
}

impl Serialize for Literal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Bool(v) => v.serialize(serializer),
            Self::Int(v) => v.serialize(serializer),
            Self::Float(v) => v.serialize(serializer),
            Self::String(v) => v.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Literal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct LiteralVisitor;

        impl Visitor<'_> for LiteralVisitor {
            type Value = Literal;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string, a number or a boolean")
            }

            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Literal, E> {
                Ok(Literal::Bool(v))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Literal, E> {
                Ok(Literal::Int(v))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Literal, E> {
                i64::try_from(v)
                    .map(Literal::Int)
                    .map_err(|_too_big| E::invalid_value(de::Unexpected::Unsigned(v), &self))
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Literal, E> {
                Finite::new(v)
                    .map(Literal::Float)
                    .ok_or_else(|| E::invalid_value(de::Unexpected::Float(v), &"a finite number"))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Literal, E> {
                Ok(Literal::String(v.to_owned()))
            }
        }

        deserializer.deserialize_any(LiteralVisitor)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Expr(String);

impl Expr {
    pub fn new(source: impl Into<String>) -> Result<Self, InvalidExpr> {
        let source = source.into();
        match cel::Program::compile(&source) {
            Ok(_) => Ok(Self(source)),
            Err(errors) => {
                let reason = errors
                    .errors
                    .first()
                    .map_or_else(|| errors.to_string(), |e| e.msg.clone());
                Err(InvalidExpr {
                    expr: source,
                    reason,
                })
            }
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Expr {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

use std::fmt;
use std::marker::PhantomData;

use serde::de::value::MapAccessDeserializer;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

/// A serde helper to store a boolean as an object.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(untagged)]
pub enum OptBoolObj<T> {
    /// No value present.
    #[default]
    #[serde(skip)]
    NoValue,
    /// A boolean value.
    Bool(bool),
    /// An object value.
    Object(T),
}

impl<T> OptBoolObj<T> {
    /// Returns `true` if this contains no value.
    pub fn is_none(&self) -> bool {
        matches!(self, Self::NoValue)
    }
}

impl<'de, T> Deserialize<'de> for OptBoolObj<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OptBoolObjVisitor<T>(PhantomData<T>);

        impl<'de, T> Visitor<'de> for OptBoolObjVisitor<T>
        where
            T: Deserialize<'de>,
        {
            type Value = OptBoolObj<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("either a boolean or a configuration map")
            }

            fn visit_unit<E: de::Error>(self) -> Result<OptBoolObj<T>, E> {
                Ok(OptBoolObj::NoValue)
            }

            fn visit_none<E: de::Error>(self) -> Result<OptBoolObj<T>, E> {
                Ok(OptBoolObj::NoValue)
            }

            fn visit_bool<E: de::Error>(self, value: bool) -> Result<OptBoolObj<T>, E> {
                Ok(OptBoolObj::Bool(value))
            }

            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<OptBoolObj<T>, M::Error> {
                let value = T::deserialize(MapAccessDeserializer::new(map))?;
                Ok(OptBoolObj::Object(value))
            }

            // Strings, numbers, and sequences fall through to serde's default, which emits a
            // located `de::Error::invalid_type` error citing this visitor's `expecting()`.
        }

        deserializer.deserialize_any(OptBoolObjVisitor(PhantomData))
    }
}

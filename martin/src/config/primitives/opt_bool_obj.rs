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

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;
    use crate::config::test_helpers::{parse_yaml, render_error};

    #[derive(Debug, Default, Deserialize, PartialEq)]
    struct Sample {
        name: String,
        #[serde(default)]
        size: u32,
    }

    // ----- Custom `Deserialize` impl: every accepted shape and every error path -----

    #[test]
    fn deserialize_null_is_no_value() {
        let cfg = parse_yaml::<OptBoolObj<Sample>>("null");
        assert_eq!(cfg, OptBoolObj::NoValue);
    }

    #[test]
    fn deserialize_bool_true() {
        let cfg = parse_yaml::<OptBoolObj<Sample>>("true");
        assert_eq!(cfg, OptBoolObj::Bool(true));
    }

    #[test]
    fn deserialize_bool_false() {
        let cfg = parse_yaml::<OptBoolObj<Sample>>("false");
        assert_eq!(cfg, OptBoolObj::Bool(false));
    }

    #[test]
    fn deserialize_object_map() {
        let cfg = parse_yaml::<OptBoolObj<Sample>>("{ name: hello, size: 7 }");
        assert_eq!(
            cfg,
            OptBoolObj::Object(Sample {
                name: "hello".to_string(),
                size: 7,
            })
        );
    }

    #[test]
    fn deserialize_rejects_string() {
        insta::assert_snapshot!(render_error::<OptBoolObj<Sample>>("hello"), @r#"
        × invalid type: string "hello", expected either a boolean or a configuration
        │ map
        "#);
    }

    #[test]
    fn deserialize_rejects_integer() {
        insta::assert_snapshot!(render_error::<OptBoolObj<Sample>>("42"), @"
        × invalid type: integer `42`, expected either a boolean or a configuration
        │ map
        ");
    }

    #[test]
    fn deserialize_rejects_sequence() {
        insta::assert_snapshot!(render_error::<OptBoolObj<Sample>>("[a, b]"), @"
        × invalid type: sequence, expected either a boolean or a configuration map
        ");
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

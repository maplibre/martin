use std::fmt;

use serde::de::value::MapAccessDeserializer;
use serde::de::{self, MapAccess, Unexpected, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A generic three-state configuration value: auto, disabled, or explicit.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum AutoOption<T> {
    /// Use the feature with its default settings.
    #[default]
    Auto,
    /// Feature is explicitly disabled.
    Disabled,
    /// Feature is enabled with explicit settings.
    Explicit(T),
}

impl<T> AutoOption<T> {
    /// Returns `true` if this is the [`Disabled`](Self::Disabled) variant.
    #[must_use]
    pub fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }

    /// Returns `true` if this is the [`Auto`](Self::Auto) variant.
    #[must_use]
    pub fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
    }

    /// Returns a reference to the explicit value, if present.
    #[must_use]
    pub fn as_explicit(&self) -> Option<&T> {
        match self {
            Self::Explicit(v) => Some(v),
            _ => None,
        }
    }
}

impl<T: Serialize> Serialize for AutoOption<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => serializer.serialize_str("auto"),
            Self::Disabled => serializer.serialize_str("disabled"),
            Self::Explicit(cfg) => cfg.serialize(serializer),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for AutoOption<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(AutoOptionVisitor(std::marker::PhantomData))
    }
}

struct AutoOptionVisitor<T>(std::marker::PhantomData<T>);

impl<'de, T: Deserialize<'de>> Visitor<'de> for AutoOptionVisitor<T> {
    type Value = AutoOption<T>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(r#"a string ("auto", "enabled", "disabled"), a boolean, or a map of settings"#)
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
        Ok(if v {
            AutoOption::Auto
        } else {
            AutoOption::Disabled
        })
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        match v {
            "auto" | "default" | "enabled" => Ok(AutoOption::Auto),
            "disabled" => Ok(AutoOption::Disabled),
            _ => Err(E::invalid_value(Unexpected::Str(v), &self)),
        }
    }

    fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
        let cfg = T::deserialize(MapAccessDeserializer::new(map))?;
        Ok(AutoOption::Explicit(cfg))
    }
}

#[cfg(feature = "unstable-schemas")]
impl<T: schemars::JsonSchema> schemars::JsonSchema for AutoOption<T> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Owned(format!("AutoOption_{}", T::schema_name()))
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let inner = generator.subschema_for::<T>();
        schemars::json_schema!({
            "description": format!(
                "Conversion configuration:\n\n\
                 - `\"auto\"`, `\"enabled\"` or boolean `true` - use defaults for this conversion\n\
                 - `\"disabled\"` or boolean `false` - disable this conversion\n\
                 - An object `{}` - explicit  settings",
                T::schema_name()
            ),
            "oneOf": [
                {
                    "type": "string",
                    "enum": ["auto","enabled"],
                    "description": "Use the feature with default settings."
                },
                {
                    "type": "string",
                    "enum": ["disabled"],
                    "description": "Disable the feature."
                },
                {
                    "type": "boolean",
                    "description": "true = auto (defaults), false = disabled."
                },
                inner,
            ]
        })
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde::Deserialize;

    use super::*;

    #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
    struct DummyCfg {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        foo: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bar: Option<u32>,
    }

    #[rstest]
    #[case("auto", AutoOption::Auto)]
    #[case("default", AutoOption::Auto)]
    #[case("true", AutoOption::Auto)]
    #[case("enabled", AutoOption::Auto)]
    #[case("disabled", AutoOption::Disabled)]
    #[case("false", AutoOption::Disabled)]
    fn parse_keyword(#[case] input: &str, #[case] expected: AutoOption<DummyCfg>) {
        let v: AutoOption<DummyCfg> = serde_yaml::from_str(input).unwrap();
        assert_eq!(v, expected);
    }

    #[test]
    fn parse_explicit() {
        let v: AutoOption<DummyCfg> = serde_yaml::from_str("foo: true\nbar: 42").unwrap();
        assert_eq!(
            v,
            AutoOption::Explicit(DummyCfg {
                foo: Some(true),
                bar: Some(42),
            })
        );
    }

    #[rstest]
    #[case("nope")]
    #[case("42")]
    fn parse_invalid(#[case] input: &str) {
        assert!(serde_yaml::from_str::<AutoOption<DummyCfg>>(input).is_err());
    }

    #[test]
    fn serde_round_trip() {
        for v in [
            AutoOption::<DummyCfg>::Auto,
            AutoOption::Disabled,
            AutoOption::Explicit(DummyCfg {
                foo: Some(true),
                bar: None,
            }),
        ] {
            let yaml = serde_yaml::to_string(&v).unwrap();
            let parsed: AutoOption<DummyCfg> = serde_yaml::from_str(&yaml).unwrap();
            assert_eq!(v, parsed);
        }
#[rstest]
 #[case::auto(AutoOption::<DummyCfg>::Auto)]
 #[case::auto(AutoOption::<DummyCfg>::Disabled)]
 #[case::auto(AutoOption::Explicit(DummyCfg {foo: Some(true),bar: None,}))]
fn serde_round_trip(v:AutoOption) {
let yaml = serde_yaml::to_string(&v).unwrap();
let parsed: AutoOption<DummyCfg> = serde_yaml::from_str(&yaml).unwrap();
assert_eq!(v, parsed);
}
}

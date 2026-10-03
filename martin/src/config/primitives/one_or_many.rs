use std::fmt;
use std::marker::PhantomData;

use serde::de::value::{MapAccessDeserializer, SeqAccessDeserializer};
use serde::de::{self, IntoDeserializer as _, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The accepted shapes of a [`Vec`] field deserialized via [`deserialize`].
#[cfg(feature = "unstable-schemas")]
#[derive(schemars::JsonSchema)]
#[serde(untagged)]
pub enum OneOrMany<T> {
    /// No values present.
    NoVals,
    /// Exactly one value present.
    One(T),
    /// Multiple values present.
    Many(Vec<T>),
}

/// Serializes the values as a sequence.
pub fn serialize<T: Serialize, S: Serializer>(
    values: &[T],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    values.serialize(serializer)
}

/// Deserializes nothing, a single value, or a sequence of values into a [`Vec`].
pub fn deserialize<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    struct OneOrManyVisitor<T>(PhantomData<T>);

    impl<'de, T> Visitor<'de> for OneOrManyVisitor<T>
    where
        T: Deserialize<'de>,
    {
        type Value = Vec<T>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("nothing, a single value, or a sequence of values")
        }

        fn visit_unit<E: de::Error>(self) -> Result<Vec<T>, E> {
            Ok(Vec::new())
        }

        fn visit_none<E: de::Error>(self) -> Result<Vec<T>, E> {
            Ok(Vec::new())
        }

        fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Vec<T>, D::Error> {
            d.deserialize_any(self)
        }

        fn visit_seq<S: SeqAccess<'de>>(self, seq: S) -> Result<Vec<T>, S::Error> {
            Deserialize::deserialize(SeqAccessDeserializer::new(seq))
        }

        fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<Vec<T>, M::Error> {
            T::deserialize(MapAccessDeserializer::new(map)).map(|v| vec![v])
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Vec<T>, E> {
            T::deserialize(value.into_deserializer()).map(|v| vec![v])
        }

        fn visit_string<E: de::Error>(self, value: String) -> Result<Vec<T>, E> {
            self.visit_str(&value)
        }

        fn visit_bool<E: de::Error>(self, value: bool) -> Result<Vec<T>, E> {
            T::deserialize(value.into_deserializer()).map(|v| vec![v])
        }

        fn visit_i64<E: de::Error>(self, value: i64) -> Result<Vec<T>, E> {
            T::deserialize(value.into_deserializer()).map(|v| vec![v])
        }

        fn visit_u64<E: de::Error>(self, value: u64) -> Result<Vec<T>, E> {
            T::deserialize(value.into_deserializer()).map(|v| vec![v])
        }

        fn visit_f64<E: de::Error>(self, value: f64) -> Result<Vec<T>, E> {
            T::deserialize(value.into_deserializer()).map(|v| vec![v])
        }
    }

    deserializer.deserialize_any(OneOrManyVisitor(PhantomData))
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use crate::config::test_helpers::parse_yaml;
    #[cfg(feature = "postgres")]
    use crate::config::test_helpers::render_failure;

    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(bound(deserialize = "T: Deserialize<'de>"))]
    struct Values<T>(#[serde(deserialize_with = "super::deserialize")] Vec<T>);

    fn parse<T: serde::de::DeserializeOwned>(yaml: &str) -> Vec<T> {
        parse_yaml::<Values<T>>(yaml).0
    }

    #[test]
    fn deserialize_null_is_empty() {
        assert_eq!(parse::<String>("null"), Vec::<String>::new());
    }

    #[test]
    fn deserialize_string_is_one() {
        assert_eq!(parse::<String>("hello"), vec!["hello".to_owned()]);
    }

    #[test]
    fn deserialize_quoted_string_is_one() {
        assert_eq!(
            parse::<String>("\"hello world\""),
            vec!["hello world".to_owned()]
        );
    }

    #[test]
    fn deserialize_empty_seq_is_empty() {
        assert_eq!(parse::<String>("[]"), Vec::<String>::new());
    }

    #[test]
    fn deserialize_singleton_seq_is_one() {
        assert_eq!(parse::<String>("[only]"), vec!["only".to_owned()]);
    }

    #[test]
    fn deserialize_multi_seq_is_many() {
        assert_eq!(
            parse::<String>("[a, b, c]"),
            vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]
        );
    }

    #[test]
    #[cfg(feature = "postgres")]
    fn deserialize_postgres_connection_string_seq_fails() {
        insta::assert_snapshot!(
            render_failure(indoc::indoc! {"
                postgres:
                  connection_string:
                    - first
                    - second
            "}),
            @"
        martin::config::yaml (https://maplibre.org/martin/config-file/)

          × expected string scalar
           ╭─[config.yaml:3:5]
         2 │   connection_string:
         3 │     - first
           ·     ┬
           ·     ╰── expected string scalar
         4 │     - second
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        "
        );
    }

    #[test]
    fn deserialize_scalars_are_one() {
        assert_eq!(parse::<bool>("true"), vec![true]);
        assert_eq!(parse::<i64>("-5"), vec![-5]);
        assert_eq!(parse::<u64>("7"), vec![7]);
        assert_eq!(parse::<f64>("1.5"), vec![1.5]);
        assert_eq!(parse::<i64>("[1, 2]"), vec![1, 2]);
    }

    #[test]
    fn deserialize_map_is_one() {
        #[derive(Debug, PartialEq, Deserialize)]
        struct Inner {
            name: String,
        }
        assert_eq!(
            parse::<Inner>("name: hello"),
            vec![Inner {
                name: "hello".to_owned()
            }]
        );
    }

    #[test]
    fn deserialize_mismatched_scalar_fails() {
        let err = serde_saphyr::from_str::<Values<bool>>("hello").unwrap_err();
        assert!(err.to_string().contains("bool"), "{err}");
    }
}

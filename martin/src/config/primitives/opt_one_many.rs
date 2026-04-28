use std::fmt;
use std::marker::PhantomData;
use std::vec::IntoIter;

use serde::de::value::{MapAccessDeserializer, SeqAccessDeserializer};
use serde::de::{self, IntoDeserializer as _, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

/// An enum that can hold no values, one value, or many values of type T.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum OptOneMany<T> {
    /// No values present.
    #[default]
    NoVals,
    /// Exactly one value present.
    One(T),
    /// Multiple values present.
    Many(Vec<T>),
}

impl<'de, T> Deserialize<'de> for OptOneMany<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OptOneManyVisitor<T>(PhantomData<T>);

        impl<'de, T> Visitor<'de> for OptOneManyVisitor<T>
        where
            T: Deserialize<'de>,
        {
            type Value = OptOneMany<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("nothing, a single value, or a sequence of values")
            }

            fn visit_unit<E: de::Error>(self) -> Result<OptOneMany<T>, E> {
                Ok(OptOneMany::NoVals)
            }

            fn visit_none<E: de::Error>(self) -> Result<OptOneMany<T>, E> {
                Ok(OptOneMany::NoVals)
            }

            fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<OptOneMany<T>, D::Error> {
                d.deserialize_any(self)
            }

            fn visit_seq<S: SeqAccess<'de>>(self, seq: S) -> Result<OptOneMany<T>, S::Error> {
                let vec: Vec<T> = Deserialize::deserialize(SeqAccessDeserializer::new(seq))?;
                Ok(OptOneMany::new(vec))
            }

            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<OptOneMany<T>, M::Error> {
                let value = T::deserialize(MapAccessDeserializer::new(map))?;
                Ok(OptOneMany::One(value))
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<OptOneMany<T>, E> {
                let value = T::deserialize(value.into_deserializer())?;
                Ok(OptOneMany::One(value))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<OptOneMany<T>, E> {
                self.visit_str(&value)
            }

            fn visit_bool<E: de::Error>(self, value: bool) -> Result<OptOneMany<T>, E> {
                let value = T::deserialize(value.into_deserializer())?;
                Ok(OptOneMany::One(value))
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<OptOneMany<T>, E> {
                let value = T::deserialize(value.into_deserializer())?;
                Ok(OptOneMany::One(value))
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<OptOneMany<T>, E> {
                let value = T::deserialize(value.into_deserializer())?;
                Ok(OptOneMany::One(value))
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<OptOneMany<T>, E> {
                let value = T::deserialize(value.into_deserializer())?;
                Ok(OptOneMany::One(value))
            }
        }

        deserializer.deserialize_any(OptOneManyVisitor(PhantomData))
    }
}

impl<T> IntoIterator for OptOneMany<T> {
    type Item = T;
    type IntoIter = IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            Self::NoVals => Vec::new().into_iter(),
            Self::One(v) => vec![v].into_iter(),
            Self::Many(v) => v.into_iter(),
        }
    }
}

impl<T> OptOneMany<T> {
    /// Creates a new `OptOneMany` from an iterator.
    ///
    /// Returns `NoVals` if the iterator is empty, `One` if it contains exactly one item,
    /// and `Many` if it contains multiple items.
    pub fn new<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut iter = iter.into_iter();
        match (iter.next(), iter.next()) {
            (Some(first), Some(second)) => {
                let mut vec = Vec::with_capacity(iter.size_hint().0 + 2);
                vec.push(first);
                vec.push(second);
                vec.extend(iter);
                Self::Many(vec)
            }
            (Some(first), None) => Self::One(first),
            (None, _) => Self::NoVals,
        }
    }

    /// Returns `true` if this contains no values.
    pub fn is_none(&self) -> bool {
        matches!(self, Self::NoVals)
    }

    /// Returns `true` if this contains no values or an empty vector.
    pub fn is_empty(&self) -> bool {
        match self {
            Self::NoVals => true,
            Self::One(_) => false,
            Self::Many(v) => v.is_empty(),
        }
    }

    /// Returns an iterator over the contained values.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        match self {
            Self::NoVals => [].iter(),
            Self::One(v) => std::slice::from_ref(v).iter(),
            Self::Many(v) => v.iter(),
        }
    }

    /// Returns an optional iterator over the contained values.
    ///
    /// Returns `None` for `NoVals`, `Some(iterator)` for `One` and `Many`.
    pub fn opt_iter(&self) -> Option<impl Iterator<Item = &T>> {
        match self {
            Self::NoVals => None,
            Self::One(v) => Some(std::slice::from_ref(v).iter()),
            Self::Many(v) => Some(v.iter()),
        }
    }

    /// Returns a mutable iterator over the contained values.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        match self {
            Self::NoVals => [].iter_mut(),
            Self::One(v) => std::slice::from_mut(v).iter_mut(),
            Self::Many(v) => v.iter_mut(),
        }
    }

    /// Returns a slice view of the contained values.
    pub fn as_slice(&self) -> &[T] {
        match self {
            Self::NoVals => &[],
            Self::One(item) => std::slice::from_ref(item),
            Self::Many(v) => v.as_slice(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OptOneMany::{Many, NoVals, One};
    use super::*;
    use crate::config::test_helpers::parse_yaml;
    #[cfg(feature = "postgres")]
    use crate::config::test_helpers::render_failure;

    // ----- Custom `Deserialize` impl: every accepted shape and every error path -----
    //
    // Success cases use `parse_yaml::<OptOneMany<String>>` directly. Failure cases run
    // through the full `parse_config` pipeline so the snapshot shows the same diagnostic
    // a user sees: in production `OptOneMany<PostgresConfig>` is the `postgres` field on
    // `Config`, so we use `postgres: …` as the wrapping context.

    #[test]
    fn deserialize_null_is_no_vals() {
        let cfg = parse_yaml::<OptOneMany<String>>("null");
        assert_eq!(cfg, NoVals);
    }

    #[test]
    fn deserialize_string_is_one() {
        let cfg = parse_yaml::<OptOneMany<String>>("hello");
        assert_eq!(cfg, One("hello".to_string()));
    }

    #[test]
    fn deserialize_quoted_string_is_one() {
        let cfg = parse_yaml::<OptOneMany<String>>("\"hello world\"");
        assert_eq!(cfg, One("hello world".to_string()));
    }

    #[test]
    fn deserialize_empty_seq_is_no_vals() {
        let cfg = parse_yaml::<OptOneMany<String>>("[]");
        assert_eq!(cfg, NoVals);
    }

    #[test]
    fn deserialize_singleton_seq_is_one() {
        let cfg = parse_yaml::<OptOneMany<String>>("[only]");
        assert_eq!(cfg, One("only".to_string()));
    }

    #[test]
    fn deserialize_multi_seq_is_many() {
        let cfg = parse_yaml::<OptOneMany<String>>("[a, b, c]");
        assert_eq!(
            cfg,
            Many(vec!["a".to_string(), "b".to_string(), "c".to_string()])
        );
    }

    #[test]
    #[cfg(feature = "postgres")]
    fn deserialize_postgres_connection_string_seq_fails() {
        // The `postgres` field on `Config` is `OptOneMany<PostgresConfig>`. Giving the
        // inner `connection_string` field a sequence (instead of a string) walks
        // `OptOneMany`'s `visit_map` → `PostgresConfig::deserialize` → fails on the
        // string field with a located span pointing at the offending sequence.
        insta::assert_snapshot!(
            render_failure(indoc::indoc! {"
                postgres:
                  connection_string:
                    - first
                    - second
            "}),
            @"
         × unexpected event: expected string scalar
          ╭─[config.yaml:3:5]
        2 │   connection_string:
        3 │     - first
          ·     ┬
          ·     ╰── unexpected event: expected string scalar
        4 │     - second
          ╰────
        "
        );
    }

    // ----- Existing behavior tests -----

    #[test]
    fn test_one_or_many_new() {
        assert_eq!(OptOneMany::new(vec![1, 2, 3]), Many(vec![1, 2, 3]));
        assert_eq!(OptOneMany::new(vec![1]), One(1));
        assert_eq!(OptOneMany::new(Vec::<i32>::new()), NoVals);
    }

    #[test]
    fn test_one_or_many_iter() {
        let mut noval: OptOneMany<i32> = NoVals;
        let mut one = One(1);
        let mut many = Many(vec![1, 2, 3]);

        assert_eq!(noval.iter_mut().collect::<Vec<_>>(), Vec::<&i32>::new());
        assert_eq!(one.iter_mut().collect::<Vec<_>>(), vec![&1]);
        assert_eq!(many.iter_mut().collect::<Vec<_>>(), vec![&1, &2, &3]);

        assert_eq!(noval.iter().collect::<Vec<_>>(), Vec::<&i32>::new());
        assert_eq!(one.iter().collect::<Vec<_>>(), vec![&1]);
        assert_eq!(many.iter().collect::<Vec<_>>(), vec![&1, &2, &3]);

        assert_eq!(noval.opt_iter().map(Iterator::collect::<Vec<_>>), None);
        assert_eq!(one.opt_iter().map(Iterator::collect), Some(vec![&1]));
        assert_eq!(
            many.opt_iter().map(Iterator::collect),
            Some(vec![&1, &2, &3])
        );

        assert_eq!(noval.into_iter().collect::<Vec<_>>(), Vec::<i32>::new());
        assert_eq!(one.into_iter().collect::<Vec<_>>(), vec![1]);
        assert_eq!(many.into_iter().collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn test_one_or_many_as_slice() {
        let noval: OptOneMany<i32> = NoVals;
        assert_eq!(noval.as_slice(), Vec::<i32>::new().as_slice());
        assert_eq!(One(1).as_slice(), &[1]);
        assert_eq!(Many(vec![1, 2, 3]).as_slice(), &[1, 2, 3]);
    }
}

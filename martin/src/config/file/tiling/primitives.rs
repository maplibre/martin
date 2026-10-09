use std::fmt;
use std::marker::PhantomData;

use serde::de::value::MapAccessDeserializer;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};

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

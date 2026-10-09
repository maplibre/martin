use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use indexmap::IndexMap;
use serde::de::value::MapAccessDeserializer;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::error::TilingConfigError;
use super::primitives::{Expr, Finite, forward_scalars, single_entry_map};
use super::value::{Lookup, Match, Ref};
use super::zoom::{Zoom, ZoomRange};

macro_rules! measure {
    ($name:ident, $unit:literal, $expecting:literal) => {
        #[derive(Clone, Copy, PartialEq, PartialOrd, Serialize)]
        pub struct $name(Finite);

        impl $name {
            pub fn new(value: f64) -> Option<Self> {
                Finite::new(value).filter(|v| v.get() >= 0.0).map(Self)
            }

            #[must_use]
            pub fn get(self) -> f64 {
                self.0.get()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!("{}", $unit), self.get())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = f64::deserialize(deserializer)?;
                Self::new(value).ok_or_else(|| {
                    de::Error::invalid_value(de::Unexpected::Float(value), &$expecting)
                })
            }
        }
    };
}

measure!(Pixels, "px", "a finite number of pixels, 0 or more");
measure!(Meters, "m", "a finite number of metres, 0 or more");

#[derive(Clone, PartialEq)]
pub enum ByZoom<U> {
    Constant(U),
    Steps(BTreeMap<Zoom, U>),
}

impl<U: fmt::Debug> fmt::Debug for ByZoom<U> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Constant(v) => fmt::Debug::fmt(v, f),
            Self::Steps(steps) => f.debug_tuple("Steps").field(steps).finish(),
        }
    }
}

impl<U: Serialize> Serialize for ByZoom<U> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Constant(v) => v.serialize(serializer),
            Self::Steps(steps) => steps.serialize(serializer),
        }
    }
}

impl<'de, U: Deserialize<'de>> Deserialize<'de> for ByZoom<U> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ByZoomVisitor<U>(PhantomData<U>);

        impl<'de, U: Deserialize<'de>> Visitor<'de> for ByZoomVisitor<U> {
            type Value = ByZoom<U>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a number, or zoom steps such as `{ 0: 2, 11: 0 }`")
            }

            forward_scalars!(U => ByZoom::Constant; visit_u64: u64, visit_i64: i64, visit_f64: f64);

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<ByZoom<U>, A::Error> {
                let steps = BTreeMap::<Zoom, U>::deserialize(MapAccessDeserializer::new(map))?;
                if steps.is_empty() {
                    return Err(de::Error::invalid_length(0, &"at least one zoom step"));
                }
                Ok(ByZoom::Steps(steps))
            }
        }

        deserializer.deserialize_any(ByZoomVisitor(PhantomData))
    }
}

pub trait FromZoomSteps: Sized + Serialize + for<'de> Deserialize<'de> {
    fn from_zoom_steps(steps: BTreeMap<Zoom, Pixels>) -> Result<Self, TilingConfigError>;
}

impl FromZoomSteps for Zoom {
    fn from_zoom_steps(_: BTreeMap<Zoom, Pixels>) -> Result<Self, TilingConfigError> {
        Err(TilingConfigError::ZoomStepsForZoom)
    }
}

impl FromZoomSteps for ByZoom<Pixels> {
    fn from_zoom_steps(steps: BTreeMap<Zoom, Pixels>) -> Result<Self, TilingConfigError> {
        Ok(Self::Steps(steps))
    }
}

#[derive(Clone, PartialEq)]
pub enum PerFeature<T> {
    Fixed(T),
    Match(Match<Self>),
    Lookup(Lookup<Self>),
    Expr(Expr),
}

pub type ZoomSetting = PerFeature<Zoom>;
pub type PixelSetting = PerFeature<ByZoom<Pixels>>;

impl<T> PerFeature<T> {
    #[must_use]
    pub fn fixed(&self) -> Option<&T> {
        match self {
            Self::Fixed(v) => Some(v),
            Self::Match(_) | Self::Lookup(_) | Self::Expr(_) => None,
        }
    }
}

pub(super) fn fixed_zoom_range(
    minzoom: Option<&ZoomSetting>,
    maxzoom: Option<&ZoomSetting>,
) -> Result<ZoomRange, TilingConfigError> {
    let fixed = |setting: Option<&ZoomSetting>| setting.and_then(PerFeature::fixed).copied();
    ZoomRange::new(fixed(minzoom), fixed(maxzoom))
}

impl<T: fmt::Debug> fmt::Debug for PerFeature<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fixed(v) => fmt::Debug::fmt(v, f),
            Self::Match(m) => fmt::Debug::fmt(m, f),
            Self::Lookup(l) => fmt::Debug::fmt(l, f),
            Self::Expr(e) => fmt::Debug::fmt(e, f),
        }
    }
}

impl<T: Serialize> Serialize for PerFeature<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Fixed(v) => v.serialize(serializer),
            Self::Match(m) => single_entry_map(serializer, "match", m),
            Self::Lookup(l) => l.serialize(serializer),
            Self::Expr(e) => single_entry_map(serializer, "expr", e),
        }
    }
}

enum SettingKey {
    Zoom(Zoom),
    Keyword(String),
}

impl<'de> Deserialize<'de> for SettingKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SettingKeyVisitor;

        impl Visitor<'_> for SettingKeyVisitor {
            type Value = SettingKey;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a zoom or one of `match`, `lookup`, `map`, `else`, `expr`")
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<SettingKey, E> {
                let zoom = u8::try_from(v).ok().and_then(Zoom::new);
                zoom.map(SettingKey::Zoom)
                    .ok_or_else(|| Zoom::out_of_range(de::Unexpected::Unsigned(v)))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<SettingKey, E> {
                let v = u64::try_from(v)
                    .map_err(|_negative| Zoom::out_of_range(de::Unexpected::Signed(v)))?;
                self.visit_u64(v)
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<SettingKey, E> {
                Ok(SettingKey::Keyword(v.to_owned()))
            }
        }

        deserializer.deserialize_any(SettingKeyVisitor)
    }
}

impl<'de, T: FromZoomSteps> Deserialize<'de> for PerFeature<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PerFeatureVisitor<T>(PhantomData<T>);

        impl<'de, T: FromZoomSteps> Visitor<'de> for PerFeatureVisitor<T> {
            type Value = PerFeature<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a number, zoom steps such as `{ 0: 2, 11: 0 }`, or `match`, `lookup` or `expr`")
            }

            forward_scalars!(T => PerFeature::Fixed; visit_u64: u64, visit_i64: i64, visit_f64: f64);

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<PerFeature<T>, A::Error> {
                let mut steps = BTreeMap::new();
                let mut r#match = None;
                let mut lookup: Option<Ref> = None;
                let mut table: Option<IndexMap<String, PerFeature<T>>> = None;
                let mut otherwise = None;
                let mut expr = None;
                let mut keywords = Vec::new();
                while let Some(key) = map.next_key::<SettingKey>()? {
                    match (&key, steps.keys().next(), keywords.first()) {
                        (SettingKey::Zoom(_), _, Some(keyword))
                        | (SettingKey::Keyword(keyword), Some(_), _) => {
                            return Err(de::Error::custom(TilingConfigError::ZoomStepsMixedWith(
                                keyword.clone(),
                            )));
                        }
                        _ => {}
                    }
                    match key {
                        SettingKey::Zoom(zoom) => {
                            steps.insert(zoom, map.next_value::<Pixels>()?);
                        }
                        SettingKey::Keyword(keyword) => {
                            match keyword.as_str() {
                                "match" => r#match = Some(map.next_value()?),
                                "lookup" => lookup = Some(map.next_value()?),
                                "map" => table = Some(map.next_value()?),
                                "else" => otherwise = Some(map.next_value()?),
                                "expr" => expr = Some(map.next_value()?),
                                _ => {
                                    return Err(de::Error::unknown_field(
                                        &keyword,
                                        &["match", "lookup", "map", "else", "expr"],
                                    ));
                                }
                            }
                            keywords.push(keyword);
                        }
                    }
                }
                if !steps.is_empty() {
                    return T::from_zoom_steps(steps)
                        .map(PerFeature::Fixed)
                        .map_err(de::Error::custom);
                }
                if table.is_some() && lookup.is_none() {
                    return Err(de::Error::custom(TilingConfigError::MapWithoutLookup));
                }
                if otherwise.is_some() && lookup.is_none() {
                    return Err(de::Error::custom(TilingConfigError::ElseWithoutLookup));
                }
                match (r#match, lookup, expr) {
                    (Some(m), None, None) => Ok(PerFeature::Match(m)),
                    (None, Some(subject), None) => Lookup::new(subject, table, otherwise)
                        .map(PerFeature::Lookup)
                        .map_err(de::Error::custom),
                    (None, None, Some(e)) => Ok(PerFeature::Expr(e)),
                    (None, None, None) => Err(de::Error::custom(TilingConfigError::NoSetting)),
                    _ => Err(de::Error::custom(TilingConfigError::SeveralSettings)),
                }
            }
        }

        deserializer.deserialize_any(PerFeatureVisitor(PhantomData))
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use crate::config::file::tiling::tests::parse;

    #[test]
    fn zooms_are_numbers_or_vary_per_feature() {
        let layers = parse(indoc! {"
            transportation:
              minzoom:
                match:
                  - { if: { highway: motorway }, value: 4 }
                  - else: { lookup: let.class, map: { minor: 12, path: 13 }, else: 14 }
              maxzoom: { expr: 'area > 1e6 ? 8 : 12' }
        "});
        let layer = layers.get("transportation").unwrap();
        insta::assert_debug_snapshot!((&layer.minzoom, &layer.maxzoom), @r#"
        (
            Some(
                Match {
                    cases: [
                        Case {
                            when: Condition {
                                highway: OneOf(
                                    [
                                        "motorway",
                                    ],
                                ),
                            },
                            then: z4,
                        },
                    ],
                    otherwise: Some(
                        Lookup {
                            subject: let.class,
                            table: {
                                "minor": z12,
                                "path": z13,
                            },
                            otherwise: Some(
                                z14,
                            ),
                        },
                    ),
                },
            ),
            Some(
                Expr("area > 1e6 ? 8 : 12"),
            ),
        )
        "#);
    }

    #[test]
    fn pixel_settings_are_numbers_zoom_steps_or_vary_per_feature() {
        let layers = parse(indoc! {"
            transportation:
              simplify: 1
              simplify_at_maxzoom: 0.0625
              min_size:
                match:
                  - { if: { route: ferry }, value: { 0: 32, 10: 0 } }
                  - else: 0.5
              min_size_at_maxzoom: 0
        "});
        let layer = layers.get("transportation").unwrap();
        insta::assert_debug_snapshot!(
            (&layer.simplify, layer.simplify_at_maxzoom, &layer.min_size, layer.min_size_at_maxzoom),
            @r#"
        (
            Some(
                1px,
            ),
            Some(
                0.0625px,
            ),
            Some(
                Match {
                    cases: [
                        Case {
                            when: Condition {
                                route: OneOf(
                                    [
                                        "ferry",
                                    ],
                                ),
                            },
                            then: Steps(
                                {
                                    z0: 32px,
                                    z10: 0px,
                                },
                            ),
                        },
                    ],
                    otherwise: Some(
                        0.5px,
                    ),
                },
            ),
            Some(
                0px,
            ),
        )
        "#
        );
    }
}

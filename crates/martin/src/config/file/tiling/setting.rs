use std::fmt;
use std::marker::PhantomData;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::config::file::tiling::error::TilingConfigError;
use crate::config::file::tiling::primitives::{Expr, Finite, forward_scalars};
use crate::config::file::tiling::zoom::{Zoom, ZoomRange};

macro_rules! measure {
    ($name:ident, $expecting:literal) => {
        #[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize)]
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

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct MeasureVisitor;

                impl Visitor<'_> for MeasureVisitor {
                    type Value = $name;

                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        f.write_str($expecting)
                    }

                    #[expect(clippy::cast_precision_loss)]
                    fn visit_u64<E: de::Error>(self, v: u64) -> Result<$name, E> {
                        self.visit_f64(v as f64)
                    }

                    #[expect(clippy::cast_precision_loss)]
                    fn visit_i64<E: de::Error>(self, v: i64) -> Result<$name, E> {
                        self.visit_f64(v as f64)
                    }

                    fn visit_f64<E: de::Error>(self, v: f64) -> Result<$name, E> {
                        $name::new(v).ok_or_else(|| E::invalid_value(de::Unexpected::Float(v), &self))
                    }
                }

                deserializer.deserialize_any(MeasureVisitor)
            }
        }
    };
}

measure!(Pixels, "a finite number of pixels, 0 or more");

#[derive(Clone, Debug, PartialEq)]
pub enum PerFeature<T> {
    Fixed(T),
    Expr(Expr),
}

pub type ZoomSetting = PerFeature<Zoom>;

impl<T> PerFeature<T> {
    #[must_use]
    pub fn fixed(&self) -> Option<&T> {
        match self {
            Self::Fixed(v) => Some(v),
            Self::Expr(_) => None,
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

impl<T: Serialize> Serialize for PerFeature<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Fixed(v) => v.serialize(serializer),
            Self::Expr(e) => e.serialize(serializer),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for PerFeature<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PerFeatureVisitor<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for PerFeatureVisitor<T> {
            type Value = PerFeature<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a number or an expression")
            }

            forward_scalars!(T => PerFeature::Fixed; visit_u64: u64, visit_i64: i64, visit_f64: f64);

            fn visit_str<E: de::Error>(self, v: &str) -> Result<PerFeature<T>, E> {
                Expr::new(v).map(PerFeature::Expr).map_err(E::custom)
            }
        }

        deserializer.deserialize_any(PerFeatureVisitor(PhantomData))
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::{PerFeature, Pixels};
    use crate::config::file::tiling::tests::parse;
    use crate::config::file::tiling::{Expr, Zoom};

    fn zoom(z: u8) -> Zoom {
        Zoom::new(z).expect("valid zoom")
    }

    fn px(value: f64) -> Pixels {
        Pixels::new(value).expect("valid pixels")
    }

    #[test]
    fn zooms_are_numbers_or_vary_per_feature() {
        let layers = parse(indoc! {"
            transportation:
              minzoom: \"highway == 'motorway' ? 4 : 12\"
              maxzoom: 14
        "});
        let layer = layers.get("transportation").expect("layer exists");
        assert_eq!(
            layer.minzoom,
            Some(PerFeature::Expr(
                Expr::new("highway == 'motorway' ? 4 : 12").expect("valid CEL")
            ))
        );
        assert_eq!(layer.maxzoom, Some(PerFeature::Fixed(zoom(14))));
    }

    #[test]
    fn pixel_settings_may_differ_at_maxzoom() {
        let layers = parse(indoc! {"
            transportation:
              simplify: 2
              simplify_at_maxzoom: 0.0625
              min_size: 0.5
              min_size_at_maxzoom: 0
        "});
        let layer = layers.get("transportation").expect("layer exists");
        assert_eq!(layer.simplify, Some(px(2.0)));
        assert_eq!(layer.simplify_at_maxzoom(), Some(px(0.0625)));
        assert_eq!(layer.min_size, Some(px(0.5)));
        assert_eq!(layer.min_size_at_maxzoom(), Some(px(0.0)));
    }

    #[test]
    fn pixel_settings_at_maxzoom_default_to_the_other_zooms() {
        let layers = parse(indoc! {"
            transportation:
              simplify: 2
              min_size: 0.5
        "});
        let layer = layers.get("transportation").expect("layer exists");
        assert_eq!(layer.simplify_at_maxzoom(), Some(px(2.0)));
        assert_eq!(layer.min_size_at_maxzoom(), Some(px(0.5)));
    }
}

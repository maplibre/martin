//! How lossy encoded images are, shared by rendered styles and re-encoded COG tiles.

use std::fmt;
use std::num::NonZeroU8;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Quality of a lossy encode, 1 (smallest) to 100 (best).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct LossyQuality(NonZeroU8);

impl LossyQuality {
    /// The quality, if `quality` is 1 to 100.
    #[must_use]
    pub const fn new(quality: u8) -> Option<Self> {
        match NonZeroU8::new(quality) {
            Some(quality) if quality.get() <= 100 => Some(Self(quality)),
            _ => None,
        }
    }

    /// The quality, 1 to 100.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0.get()
    }
}

impl Default for LossyQuality {
    fn default() -> Self {
        Self::new(80).expect("80 is a quality")
    }
}

/// How lossy a WebP is encoded.
///
/// Deserializes from a number 1 to 100 or the string `lossless`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageQuality {
    /// Keep every pixel.
    Lossless,
    /// Lossy at this quality.
    Lossy(LossyQuality),
}

impl Default for ImageQuality {
    fn default() -> Self {
        Self::Lossy(LossyQuality::default())
    }
}

impl Serialize for ImageQuality {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Lossless => serializer.serialize_str("lossless"),
            Self::Lossy(quality) => quality.serialize(serializer),
        }
    }
}

/// The quality `quality` is, or an error naming the `allowed` values.
fn lossy<E: de::Error>(quality: u64, allowed: &str) -> Result<LossyQuality, E> {
    u8::try_from(quality)
        .ok()
        .and_then(LossyQuality::new)
        .ok_or_else(|| E::custom(format_args!("quality must be {allowed}, not {quality}")))
}

const WEBP_QUALITIES: &str = r#"1 to 100 or "lossless""#;

impl<'de> Deserialize<'de> for LossyQuality {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct LossyVisitor;

        impl Visitor<'_> for LossyVisitor {
            type Value = LossyQuality;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a quality from 1 to 100")
            }

            fn visit_u64<E: de::Error>(self, quality: u64) -> Result<LossyQuality, E> {
                lossy(quality, "1 to 100")
            }
        }

        deserializer.deserialize_any(LossyVisitor)
    }
}

impl<'de> Deserialize<'de> for ImageQuality {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct QualityVisitor;

        impl Visitor<'_> for QualityVisitor {
            type Value = ImageQuality;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str(r#"a quality from 1 to 100, or "lossless""#)
            }

            fn visit_u64<E: de::Error>(self, quality: u64) -> Result<ImageQuality, E> {
                lossy(quality, WEBP_QUALITIES).map(ImageQuality::Lossy)
            }

            fn visit_str<E: de::Error>(self, quality: &str) -> Result<ImageQuality, E> {
                if quality == "lossless" {
                    Ok(ImageQuality::Lossless)
                } else {
                    Err(E::custom(format_args!(
                        r#"quality must be {WEBP_QUALITIES}, not "{quality}""#
                    )))
                }
            }
        }

        deserializer.deserialize_any(QualityVisitor)
    }
}

#[cfg(feature = "unstable-schemas")]
impl schemars::JsonSchema for LossyQuality {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("LossyQuality")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "Lossy quality from 1 to 100.",
            "type": "integer",
            "minimum": 1,
            "maximum": 100
        })
    }
}

#[cfg(feature = "unstable-schemas")]
impl schemars::JsonSchema for ImageQuality {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ImageQuality")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "Lossy quality from 1 to 100, or `lossless`.",
            "oneOf": [
                { "type": "integer", "minimum": 1, "maximum": 100 },
                { "const": "lossless" }
            ]
        })
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::zero(0, None)]
    #[case::lowest(1, Some(1))]
    #[case::highest(100, Some(100))]
    #[case::above_100(101, None)]
    fn a_lossy_quality_is_1_to_100(#[case] quality: u8, #[case] expected: Option<u8>) {
        assert_eq!(LossyQuality::new(quality).map(LossyQuality::get), expected);
    }
}

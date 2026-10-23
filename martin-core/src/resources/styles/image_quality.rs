use std::fmt;
use std::num::NonZeroU8;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Quality of a lossy encode, 1 (smallest) to 100 (best).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

/// Quality rendered images are encoded at, unless configured otherwise.
pub const DEFAULT_IMAGE_QUALITY: ImageQuality = ImageQuality::Lossy(LossyQuality::new(80).expect("80 is a quality"));

/// How lossy rendered images are encoded.
///
/// Deserializes from a number 1 to 100 or the string `lossless`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageQuality {
    /// Keep every pixel, where the format can.
    Lossless,
    /// Lossy at this quality.
    Lossy(LossyQuality),
}

impl Default for ImageQuality {
    fn default() -> Self {
        DEFAULT_IMAGE_QUALITY
    }
}

impl Serialize for ImageQuality {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Lossless => serializer.serialize_str("lossless"),
            Self::Lossy(quality) => serializer.serialize_u8(quality.get()),
        }
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
                u8::try_from(quality)
                    .ok()
                    .and_then(LossyQuality::new)
                    .map(ImageQuality::Lossy)
                    .ok_or_else(|| invalid(quality))
            }

            fn visit_str<E: de::Error>(self, quality: &str) -> Result<ImageQuality, E> {
                if quality == "lossless" {
                    Ok(ImageQuality::Lossless)
                } else {
                    Err(invalid(format_args!("\"{quality}\"")))
                }
            }
        }

        fn invalid<E: de::Error>(quality: impl fmt::Display) -> E {
            E::custom(format_args!(r#"quality must be 1 to 100 or "lossless", not {quality}"#))
        }

        deserializer.deserialize_any(QualityVisitor)
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

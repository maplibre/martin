use std::fmt;
use std::num::NonZeroU8;

use serde::{Deserialize, Serialize};

/// The whole-number pixel ratios (`@{n}x`) rendered tiles and static images are served at.
///
/// Each ratio needs renderers of its own, so only the listed ones are served.
/// Defaults to `@1x` and `@2x`, the ratios map clients ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<u8>", into = "Vec<u8>")]
pub struct PixelRatios(
    /// Bit `n - 1` is set if `@{n}x` is served.
    u8,
);

/// A pixel ratio list that is empty or holds a ratio outside `1..=`[`PixelRatios::MAX`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidPixelRatios {
    /// No ratio is listed.
    #[error("pixel_ratios must list at least one pixel ratio")]
    Empty,
    /// A ratio outside `1..=`[`PixelRatios::MAX`].
    #[error("pixel ratio must be 1 to {max}, got {0}", max = PixelRatios::MAX)]
    OutOfRange(u8),
}

/// A request for a pixel ratio that is not configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Pixel ratio @{requested}x is not served, only {served}")]
pub struct PixelRatioNotServed {
    requested: NonZeroU8,
    served: PixelRatios,
}

impl PixelRatios {
    /// Highest pixel ratio that can be configured.
    pub const MAX: u8 = 4;

    /// Returns `Ok` if `@{ratio}x` is served.
    ///
    /// # Errors
    ///
    /// Returns [`PixelRatioNotServed`] if it is not.
    pub fn check(self, ratio: NonZeroU8) -> Result<(), PixelRatioNotServed> {
        if ratio.get() <= Self::MAX && self.0 & (1 << (ratio.get() - 1)) != 0 {
            Ok(())
        } else {
            Err(PixelRatioNotServed {
                requested: ratio,
                served: self,
            })
        }
    }

    fn iter(self) -> impl Iterator<Item = u8> {
        (1..=Self::MAX).filter(move |ratio| self.0 & (1 << (ratio - 1)) != 0)
    }
}

impl Default for PixelRatios {
    fn default() -> Self {
        Self(0b11)
    }
}

impl TryFrom<Vec<u8>> for PixelRatios {
    type Error = InvalidPixelRatios;

    /// Collects `ratios`; a ratio listed twice is served once.
    fn try_from(ratios: Vec<u8>) -> Result<Self, Self::Error> {
        let mut mask = 0;
        for ratio in ratios {
            if !(1..=Self::MAX).contains(&ratio) {
                return Err(InvalidPixelRatios::OutOfRange(ratio));
            }
            mask |= 1 << (ratio - 1);
        }
        if mask == 0 {
            Err(InvalidPixelRatios::Empty)
        } else {
            Ok(Self(mask))
        }
    }
}

impl From<PixelRatios> for Vec<u8> {
    fn from(ratios: PixelRatios) -> Self {
        ratios.iter().collect()
    }
}

/// Lists the ratios as `@1x, @2x`.
impl fmt::Display for PixelRatios {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, ratio) in self.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "@{ratio}x")?;
        }
        Ok(())
    }
}

#[cfg(feature = "unstable-schemas")]
impl schemars::JsonSchema for PixelRatios {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("PixelRatios")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "Whole-number pixel ratios (`@{n}x`) that are served.",
            "type": "array",
            "items": { "type": "integer", "minimum": 1, "maximum": Self::MAX },
            "minItems": 1
        })
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn ratio(n: u8) -> NonZeroU8 {
        NonZeroU8::new(n).expect("non-zero")
    }

    #[test]
    fn the_default_serves_1x_and_2x() {
        let ratios = PixelRatios::default();
        assert_eq!(Vec::from(ratios), [1, 2]);
        assert_eq!(ratios.to_string(), "@1x, @2x");
    }

    #[rstest]
    #[case::one(vec![1], &[1])]
    #[case::not_contiguous(vec![4, 1], &[1, 4])]
    #[case::duplicates(vec![2, 2, 1], &[1, 2])]
    #[case::all(vec![1, 2, 3, 4], &[1, 2, 3, 4])]
    fn a_list_converts(#[case] list: Vec<u8>, #[case] served: &[u8]) {
        let ratios = PixelRatios::try_from(list).expect("valid");
        assert_eq!(Vec::from(ratios), served);
        for n in 1..=u8::MAX {
            assert_eq!(ratios.check(ratio(n)).is_ok(), served.contains(&n), "@{n}x");
        }
    }

    #[rstest]
    #[case::empty(vec![], InvalidPixelRatios::Empty)]
    #[case::zero(vec![1, 0], InvalidPixelRatios::OutOfRange(0))]
    #[case::above_max(vec![5], InvalidPixelRatios::OutOfRange(5))]
    #[case::shift_overflow(vec![9], InvalidPixelRatios::OutOfRange(9))]
    fn an_invalid_list_is_rejected(#[case] list: Vec<u8>, #[case] err: InvalidPixelRatios) {
        assert_eq!(PixelRatios::try_from(list), Err(err));
    }

    #[test]
    fn an_unserved_ratio_names_the_served_ones() {
        let ratios = PixelRatios::try_from(vec![1, 3]).expect("valid");
        assert_eq!(ratios.check(ratio(3)), Ok(()));
        let err = ratios.check(ratio(2)).expect_err("@2x is not served");
        assert_eq!(err.to_string(), "Pixel ratio @2x is not served, only @1x, @3x");
    }
}

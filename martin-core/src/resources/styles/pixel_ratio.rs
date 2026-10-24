use std::fmt;

/// Pixel ratio of a static render, snapped to steps of 0.1 within [`Self::MIN`]..=[`Self::MAX`].
///
/// Stored as whole tenths so two ratios compare exactly: requests that snap to the same
/// step share a renderer, so arbitrary scales like `@1.001x` cannot each force a rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PixelRatio(u8);

/// A pixel ratio that is not finite and positive, or snaps outside [`PixelRatio::MIN`]..=[`PixelRatio::MAX`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidPixelRatio {
    /// Zero, negative, NaN or infinite.
    #[error("pixel ratio must be a positive finite number")]
    NotPositive,
    /// Snaps below [`PixelRatio::MIN`].
    #[error("pixel ratio must be at least {}", PixelRatio::MIN)]
    BelowMin,
    /// Snaps above [`PixelRatio::MAX`].
    #[error("pixel ratio must be at most {}", PixelRatio::MAX)]
    AboveMax,
}

impl PixelRatio {
    /// `@1x`, the standard density.
    pub const ONE: Self = Self(10);
    /// Lowest ratio served, `@0.5x`.
    pub const MIN: Self = Self(5);
    /// Highest ratio served, `@4x`.
    pub const MAX: Self = Self(40);

    /// The ratio as the renderer takes it.
    #[must_use]
    pub fn as_f32(self) -> f32 {
        f32::from(self.0) / 10.0
    }
}

impl TryFrom<f32> for PixelRatio {
    type Error = InvalidPixelRatio;

    /// Rounds `ratio` to the nearest 0.1, then checks the bounds.
    fn try_from(ratio: f32) -> Result<Self, Self::Error> {
        if !ratio.is_finite() || ratio <= 0.0 {
            return Err(InvalidPixelRatio::NotPositive);
        }
        let tenths = (ratio * 10.0).round();
        if tenths < f32::from(Self::MIN.0) {
            Err(InvalidPixelRatio::BelowMin)
        } else if tenths > f32::from(Self::MAX.0) {
            Err(InvalidPixelRatio::AboveMax)
        } else {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a whole number within MIN..=MAX"
            )]
            Ok(Self(tenths as u8))
        }
    }
}

impl fmt::Display for PixelRatio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_f32().fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::one(1.0, 1.0)]
    #[case::retina(2.0, 2.0)]
    #[case::down_to_the_step(1.04, 1.0)]
    #[case::up_to_the_step(1.06, 1.1)]
    #[case::float_jitter(1.250_000_1, 1.3)]
    #[case::one_point_one(1.1, 1.1)]
    #[case::min(0.5, 0.5)]
    #[case::rounds_up_to_min(0.45, 0.5)]
    #[case::max(4.0, 4.0)]
    #[case::rounds_down_to_max(4.04, 4.0)]
    fn a_ratio_snaps_to_tenths(#[case] ratio: f32, #[case] snapped: f32) {
        let pixel_ratio = PixelRatio::try_from(ratio).expect("in range");
        assert_eq!(pixel_ratio.as_f32().to_bits(), snapped.to_bits(), "{ratio}");
    }

    #[test]
    fn ratios_that_snap_to_the_same_step_are_equal() {
        assert_eq!(PixelRatio::try_from(1.04), Ok(PixelRatio::ONE));
        assert_eq!(PixelRatio::try_from(0.96), Ok(PixelRatio::ONE));
        assert_ne!(PixelRatio::try_from(1.06), Ok(PixelRatio::ONE));
    }

    #[rstest]
    #[case::zero(0.0, InvalidPixelRatio::NotPositive)]
    #[case::negative_zero(-0.0, InvalidPixelRatio::NotPositive)]
    #[case::negative(-2.0, InvalidPixelRatio::NotPositive)]
    #[case::nan(f32::NAN, InvalidPixelRatio::NotPositive)]
    #[case::infinite(f32::INFINITY, InvalidPixelRatio::NotPositive)]
    #[case::tiny(0.05, InvalidPixelRatio::BelowMin)]
    #[case::just_below_min(0.44, InvalidPixelRatio::BelowMin)]
    #[case::just_above_max(4.06, InvalidPixelRatio::AboveMax)]
    #[case::huge(f32::MAX, InvalidPixelRatio::AboveMax)]
    fn an_out_of_range_ratio_is_rejected(#[case] ratio: f32, #[case] err: InvalidPixelRatio) {
        assert_eq!(PixelRatio::try_from(ratio), Err(err));
    }

    #[test]
    fn it_displays_as_a_decimal() {
        assert_eq!(PixelRatio::try_from(1.1).expect("in range").to_string(), "1.1");
        assert_eq!(PixelRatio::ONE.to_string(), "1");
    }
}

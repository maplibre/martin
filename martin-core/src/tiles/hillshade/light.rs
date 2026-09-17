//! The direction the terrain is lit from.

/// Where the light shines from, in the terms an operator configures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightAngles {
    /// Compass bearing the light comes from, in degrees clockwise from north.
    pub azimuth_deg: f64,
    /// Height of the light above the horizon, in degrees:
    /// - `90` is directly overhead, which flattens the relief entirely.
    /// - `0` is on the horizon and produces the longest shadows.
    pub altitude_deg: f64,
}

impl Default for LightAngles {
    /// North-west of the terrain, at 45 degrees above the horizon.
    ///
    /// Lighting terrain from the upper left is the long-standing cartographic convention:
    /// relief lit from the lower right reads as inverted to most viewers, with valleys appearing to bulge outward.
    fn default() -> Self {
        Self {
            azimuth_deg: 300.0,
            altitude_deg: 45.0,
        }
    }
}

impl LightAngles {
    /// The unit vector pointing toward the light.
    #[must_use]
    pub fn to_vector(self) -> [f64; 3] {
        let az = self.azimuth_deg.to_radians();
        let alt = self.altitude_deg.to_radians();
        let cos_alt = alt.cos();
        [az.sin() * cos_alt, az.cos() * cos_alt, alt.sin()]
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_1_SQRT_2;

    use approx::assert_abs_diff_eq;
    use rstest::rstest;

    use super::*;

    #[test]
    fn the_default_lights_terrain_from_the_north_west() {
        let got = LightAngles::default().to_vector();
        assert_abs_diff_eq!(
            got[..],
            [
                -0.612_372_435_695_794_6,
                0.353_553_390_593_273_84,
                FRAC_1_SQRT_2
            ][..],
            epsilon = 1e-9
        );
    }

    #[rstest]
    #[case::overhead(0.0, 90.0, [0.0, 0.0, 1.0])]
    #[case::north(0.0, 0.0, [0.0, 1.0, 0.0])]
    #[case::east(90.0, 0.0, [1.0, 0.0, 0.0])]
    #[case::south(180.0, 0.0, [0.0, -1.0, 0.0])]
    #[case::west(270.0, 0.0, [-1.0, 0.0, 0.0])]
    fn cardinal_angles_map_to_axes(
        #[case] azimuth_deg: f64,
        #[case] altitude_deg: f64,
        #[case] expected: [f64; 3],
    ) {
        let got = LightAngles {
            azimuth_deg,
            altitude_deg,
        }
        .to_vector();
        assert_abs_diff_eq!(got[..], expected[..], epsilon = 1e-9);
    }

    #[rstest]
    #[case(0.0, 0.0)]
    #[case(315.0, 45.0)]
    #[case(37.0, 12.5)]
    #[case(360.0, 90.0)]
    fn every_angle_yields_a_unit_vector(#[case] azimuth_deg: f64, #[case] altitude_deg: f64) {
        let [x, y, z] = LightAngles {
            azimuth_deg,
            altitude_deg,
        }
        .to_vector();
        let length = (x * x + y * y + z * z).sqrt();
        assert_abs_diff_eq!(length, 1.0, epsilon = 1e-9);
    }
}

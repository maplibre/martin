//! Decodes Mapzen Terrarium RGBA texels into a grid of elevation values.

use crate::tiles::neighbourhood::{RgbaField, TILE_SIZE};

/// Elevations at or below this read as nodata rather than terrain.
///
/// An all-zero (blank or black) Terrarium pixel decodes to the format floor,
/// -32768; near-floor encodings land just above it (B=1 gives -32767.996), so an
/// exact-equality check would miss them. The deepest real sample on Earth is about
/// -11000 m, leaving this band unambiguous.
const NODATA_CEILING_METERS: f32 = -20000.0;

/// Elevation in meters for one Terrarium texel: `(R*256 + G + B/256) - 32768`.
fn decode_terrarium(texel: [u8; 4]) -> f32 {
    let [r, g, b, _a] = texel;
    (f32::from(r) * 256.0 + f32::from(g) + f32::from(b) / 256.0) + f32::from(i16::MIN)
}

/// A grid of elevation values in meters, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct HeightGrid {
    values: Vec<f32>,
    width: usize,
    height: usize,
}

impl HeightGrid {
    /// Decodes the centre tile of `field` plus `margin` pixels of its neighbours.
    ///
    /// The margin is the fetch apron: contour lines are traced across it so a line
    /// crossing a tile edge meets its continuation in the adjacent tile instead of
    /// stopping short, and it is transformed back out before encoding.
    #[must_use]
    pub fn from_field(field: &RgbaField, margin: u8) -> Self {
        let margin = usize::from(margin);
        let side = TILE_SIZE + 2 * margin;
        let start = TILE_SIZE - margin;

        let mut values = Vec::with_capacity(side * side);
        for y in 0..side {
            for x in 0..side {
                values.push(decode_terrarium(field.texel(start + x, start + y)));
            }
        }
        Self {
            values,
            width: side,
            height: side,
        }
    }

    /// Builds a grid directly from row-major elevation values in meters.
    ///
    /// # Panics
    ///
    /// Panics if `values` does not hold exactly `width * height` samples.
    #[must_use]
    pub fn from_values(values: Vec<f32>, width: usize, height: usize) -> Self {
        assert_eq!(values.len(), width * height, "a height grid must hold width * height samples");
        Self {
            values,
            width,
            height,
        }
    }

    /// Row-major elevation values in meters.
    #[must_use]
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    /// Grid extent in samples, as `(width, height)`.
    #[must_use]
    pub fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Calculates a set of elevation thresholds to draw the contour lines at.
    ///
    /// The first threshold sits above the grid minimum.
    /// A threshold at or below it is crossed by nothing.
    /// In this case, marching squares answers with a ring around the whole grid (tile + apron)
    #[must_use]
    pub fn get_thresholds(&self, interval: f32) -> Vec<f32> {
        let Some((min_elevation, max_elevation)) = self.min_max() else {
            return vec![];
        };

        let start_threshold = ((min_elevation / interval).floor() + 1.0) * interval;

        let mut thresholds = Vec::new();
        let mut current_threshold = start_threshold;

        while current_threshold <= max_elevation {
            thresholds.push(current_threshold);
            current_threshold += interval;
        }

        thresholds
    }

    /// Finite min and max elevation over the grid, or `None` if no value qualifies.
    ///
    /// Skips anything at or below [`NODATA_CEILING_METERS`], so one all-zero pixel
    /// cannot stretch the threshold range by thousands of levels.
    fn min_max(&self) -> Option<(f32, f32)> {
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        let mut found_one = false;

        for &value in &self.values {
            if value.is_finite() && value > NODATA_CEILING_METERS {
                min = min.min(value);
                max = max.max(value);
                found_one = true;
            }
        }

        found_one.then_some((min, max))
    }
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;

    use super::*;

    /// A grid of `values`, as a single row.
    fn grid(values: &[f32]) -> HeightGrid {
        HeightGrid::from_values(values.to_vec(), values.len(), 1)
    }

    /// Terrarium floor (all-zero pixel) and the next encodable step above it.
    const FLOOR: f32 = -32768.0;
    const NEAR_FLOOR: f32 = -32768.0 + 1.0 / 256.0;

    #[test]
    fn nodata_pixel_does_not_stretch_the_threshold_range() {
        // One blank pixel among real terrain. Without the guard the range starts
        // at -32800 and yields ~330 levels at a 100 m interval.
        let poisoned = grid(&[FLOOR, 120.0, 350.0]);
        assert_eq!(poisoned.get_thresholds(100.0), vec![200.0, 300.0]);

        // Same terrain without the blank pixel: identical thresholds.
        assert_eq!(grid(&[120.0, 350.0]).get_thresholds(100.0), poisoned.get_thresholds(100.0));
    }

    #[test]
    fn near_floor_pixel_is_nodata_too() {
        // B=1 decodes just above the floor, so an exact-equality guard would let
        // it through and reopen the ~330-level blowup.
        assert_eq!(grid(&[NEAR_FLOOR, 120.0, 350.0]).get_thresholds(100.0), vec![200.0, 300.0]);
    }

    #[test]
    fn deepest_real_elevation_is_kept() {
        // Challenger Deep is the floor of real data and must survive the guard:
        // range -10935..0 at a 5000 m interval starts at -10000.
        assert_eq!(grid(&[-10935.0, 0.0]).get_thresholds(5000.0), vec![-10000.0, -5000.0, 0.0]);
    }

    #[test]
    fn a_threshold_at_or_below_the_minimum_is_dropped() {
        assert_eq!(grid(&[120.0, 350.0]).get_thresholds(100.0), vec![200.0, 300.0]);
        assert_eq!(grid(&[200.0, 350.0]).get_thresholds(100.0), vec![300.0]);
    }

    #[test]
    fn all_nodata_grid_yields_no_thresholds() {
        assert!(grid(&[FLOOR, NEAR_FLOOR]).get_thresholds(100.0).is_empty());
    }

    #[test]
    fn blank_pixel_decodes_to_the_terrarium_floor() {
        assert_relative_eq!(decode_terrarium([0, 0, 0, 0]), FLOOR);
    }

    #[test]
    fn terrarium_decode_matches_the_reference_formula() {
        assert_relative_eq!(decode_terrarium([128, 0, 0, 255]), 0.0);
        assert_relative_eq!(decode_terrarium([128, 100, 128, 255]), 100.5);
    }

    #[test]
    fn from_field_crops_to_the_centre_plus_margin() {
        let field = RgbaField::uniform([128, 0, 0, 255]);
        for margin in [0u8, 1, 32] {
            let grid = HeightGrid::from_field(&field, margin);
            let expected = TILE_SIZE + 2 * usize::from(margin);
            assert_eq!(grid.dimensions(), (expected, expected), "margin {margin}");
            assert_eq!(grid.values().len(), expected * expected, "margin {margin}");
        }
    }
}

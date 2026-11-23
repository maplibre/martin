//! Decodes Mapzen Terrarium RGBA texels into a grid of elevation values.

use crate::tiles::neighbourhood::{AncestorWindow, RgbaField};

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
    (f32::mul_add(f32::from(r), 256.0, f32::from(g)) + f32::from(b) / 256.0) + f32::from(i16::MIN)
}

/// The two field texels one grid sample reads along an axis, and how far it sits between them.
#[derive(Clone, Copy, Debug)]
struct Taps {
    /// The texel at or before the sample.
    near: usize,
    /// The texel after it, clamped at the field's edge.
    far: usize,
    /// Fraction of the way from `near` to `far`, `0` when the sample sits exactly on `near`.
    weight: f32,
}

impl Taps {
    /// Taps for `ancestor`, a pixel-centre coordinate relative to the centre tile, in a field `field_side` texels wide.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "the coordinate is clamped into the field, which is a few thousand texels wide"
    )]
    fn at(ancestor: f64, tile_size: usize, field_side: usize) -> Self {
        let last = (field_side - 1) as f64;
        let position = (ancestor + tile_size as f64).clamp(0.0, last);
        let near = position.floor();
        Self {
            near: near as usize,
            far: (near + 1.0).min(last) as usize,
            weight: (position - near) as f32,
        }
    }

    /// Interpolates from `near` towards the value `far` yields, falling back to whichever side holds data.
    ///
    /// `far` is only evaluated when the sample sits between two texels, so a native read touches one texel per sample.
    fn blend(self, near: f32, far: impl FnOnce() -> f32) -> f32 {
        if self.weight <= 0.0 {
            return near;
        }
        let far = far();
        match (is_nodata(near), is_nodata(far)) {
            (false, false) => f32::mul_add(far - near, self.weight, near),
            (true, false) => far,
            (_, true) => near,
        }
    }
}

/// Whether `meters` is a nodata sentinel rather than terrain.
fn is_nodata(meters: f32) -> bool {
    meters <= NODATA_CEILING_METERS
}

/// A grid of elevation values in meters, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct HeightGrid {
    values: Vec<f32>,
    width: usize,
    height: usize,
}

impl HeightGrid {
    /// Decodes the `window` of the field's centre tile plus `margin` pixels of its neighbours.
    ///
    /// The grid is always one tile plus its margin wide, sampled at the field's own tile size, so a 512-square source contributes all 512 rows rather than its top-left quadrant.
    /// A native window reads every texel as is.
    /// A window into an ancestor upsamples that part of it bilinearly, between the decoded elevations rather than the encoded bytes, and leans on the valid sample where one side is nodata.
    ///
    /// The margin is the fetch apron: contour lines are traced across it so a line
    /// crossing a tile edge meets its continuation in the adjacent tile instead of
    /// stopping short, and it is transformed back out before encoding.
    #[must_use]
    pub fn from_field(field: &RgbaField, margin: u8, window: AncestorWindow) -> Self {
        let margin = usize::from(margin);
        let tile_size = field.tile_size();
        let side = tile_size + 2 * margin;
        #[expect(clippy::cast_precision_loss, reason = "a grid side is a few hundred samples")]
        let axis = |map: fn(AncestorWindow, usize, f64) -> f64| -> Vec<Taps> {
            (0..side)
                .map(|i| {
                    Taps::at(
                        map(window, tile_size, i as f64 - margin as f64),
                        tile_size,
                        field.field_side(),
                    )
                })
                .collect()
        };
        let columns = axis(AncestorWindow::ancestor_x);
        let rows = axis(AncestorWindow::ancestor_y);

        let height = |x: usize, y: usize| decode_terrarium(field.texel(x, y));
        let mut values = Vec::with_capacity(side * side);
        for row in &rows {
            for column in &columns {
                let top = column.blend(height(column.near, row.near), || height(column.far, row.near));
                let value = row.blend(top, || {
                    column.blend(height(column.near, row.far), || height(column.far, row.far))
                });
                values.push(value);
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
        assert_eq!(
            values.len(),
            width * height,
            "a height grid must hold width * height samples"
        );
        Self { values, width, height }
    }

    /// Row-major elevation values in meters.
    #[must_use]
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    /// Grid extent in samples, as `(width, height)`.
    #[must_use]
    pub const fn dimensions(&self) -> (usize, usize) {
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
            if value.is_finite() && !is_nodata(value) {
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
    use std::io::Cursor;

    use approx::assert_relative_eq;
    use image::{ImageFormat, RgbaImage};
    use martin_tile_utils::TileCoord;

    use super::*;
    use crate::tiles::neighbourhood::{DEFAULT_TILE_SIZE, Neighbourhood};

    fn row_ramp_field(side: u32, row_meters: impl Fn(u32) -> Option<f32>) -> RgbaField {
        let image = RgbaImage::from_fn(side, side, |_, row| {
            let Some(meters) = row_meters(row) else {
                return image::Rgba([0, 0, 0, 0]);
            };
            let raw = meters + 32768.0;
            #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let r = (raw / 256.0).floor() as u8;
            #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let g = (raw - f32::from(r) * 256.0) as u8;
            image::Rgba([r, g, 0, 255])
        });
        let mut png = Vec::new();
        image.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).unwrap();
        Neighbourhood::centre_only(png.into()).assemble().unwrap()
    }

    fn first_column(grid: &HeightGrid) -> Vec<f32> {
        let (width, _) = grid.dimensions();
        grid.values().iter().step_by(width).copied().collect()
    }

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
        assert_eq!(
            grid(&[120.0, 350.0]).get_thresholds(100.0),
            poisoned.get_thresholds(100.0)
        );
    }

    #[test]
    fn near_floor_pixel_is_nodata_too() {
        // B=1 decodes just above the floor, so an exact-equality guard would let
        // it through and reopen the ~330-level blowup.
        assert_eq!(
            grid(&[NEAR_FLOOR, 120.0, 350.0]).get_thresholds(100.0),
            vec![200.0, 300.0]
        );
    }

    #[test]
    fn deepest_real_elevation_is_kept() {
        // Challenger Deep is the floor of real data and must survive the guard:
        // range -10935..0 at a 5000 m interval starts at -10000.
        assert_eq!(
            grid(&[-10935.0, 0.0]).get_thresholds(5000.0),
            vec![-10000.0, -5000.0, 0.0]
        );
    }

    #[test]
    fn a_threshold_at_or_below_the_minimum_is_dropped() {
        assert_eq!(grid(&[120.0, 350.0]).get_thresholds(100.0), vec![200.0, 300.0]);
        assert_eq!(grid(&[200.0, 350.0]).get_thresholds(100.0), vec![300.0]);
    }

    #[test]
    fn all_nodata_grid_yields_no_thresholds() {
        assert_eq!(grid(&[FLOOR, NEAR_FLOOR]).get_thresholds(100.0), [] as [f32; 0]);
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
        for tile_size in [DEFAULT_TILE_SIZE, 2 * DEFAULT_TILE_SIZE] {
            let field = RgbaField::uniform([128, 0, 0, 255], tile_size);
            for margin in [0u8, 1, 32] {
                let grid = HeightGrid::from_field(&field, margin, AncestorWindow::NATIVE);
                let expected = tile_size + 2 * usize::from(margin);
                assert_eq!(
                    grid.dimensions(),
                    (expected, expected),
                    "tile size {tile_size}, margin {margin}"
                );
                assert_eq!(
                    grid.values().len(),
                    expected * expected,
                    "tile size {tile_size}, margin {margin}"
                );
            }
        }
    }

    #[test]
    fn a_native_window_reads_the_centre_rows_as_encoded() {
        #[expect(clippy::cast_precision_loss)]
        let field = row_ramp_field(8, |row| Some(100.0 * row as f32));
        let grid = HeightGrid::from_field(&field, 0, AncestorWindow::NATIVE);
        assert_eq!(
            first_column(&grid),
            [0.0, 100.0, 200.0, 300.0, 400.0, 500.0, 600.0, 700.0]
        );
    }

    #[test]
    fn a_child_window_interpolates_between_its_parents_pixels() {
        #[expect(clippy::cast_precision_loss)]
        let field = row_ramp_field(8, |row| Some(100.0 * row as f32));
        let (_, south_child) = AncestorWindow::above(TileCoord::new_unchecked(1, 0, 1), 1);
        let grid = HeightGrid::from_field(&field, 0, south_child);
        assert_eq!(
            first_column(&grid),
            [375.0, 425.0, 475.0, 525.0, 575.0, 625.0, 675.0, 700.0]
        );
    }

    #[test]
    fn a_nodata_row_does_not_drag_the_interpolation_towards_the_floor() {
        #[expect(clippy::cast_precision_loss)]
        let field = row_ramp_field(8, |row| (row != 4).then_some(100.0 * row as f32));
        let (_, north_child) = AncestorWindow::above(TileCoord::new_unchecked(1, 0, 0), 1);
        let grid = HeightGrid::from_field(&field, 0, north_child);
        assert_eq!(
            first_column(&grid),
            [0.0, 25.0, 75.0, 125.0, 175.0, 225.0, 275.0, 300.0]
        );
    }

    #[test]
    fn a_child_window_keeps_the_grid_one_tile_plus_margin_wide() {
        let field = RgbaField::uniform([128, 0, 0, 255], 2 * DEFAULT_TILE_SIZE);
        let (_, window) = AncestorWindow::above(TileCoord::new_unchecked(14, 9000, 6000), 3);
        let grid = HeightGrid::from_field(&field, 32, window);
        assert_eq!(
            grid.dimensions(),
            (2 * DEFAULT_TILE_SIZE + 64, 2 * DEFAULT_TILE_SIZE + 64)
        );
    }
}

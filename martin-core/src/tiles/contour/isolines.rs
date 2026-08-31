//! Isoline generation: an elevation grid to styled contour line features.

use contour::ContourBuilder;
use geo::MapCoords as _;
use geo::algorithm::simplify::Simplify as _;
use geo_types::{Coord, Line, LineString};

use super::elevation::HeightGrid;
use super::error::ContourError;
use super::features::{ContourFeature, GeometryFeatures, GeometryTransform};
use super::mvt::MvtEncodingOptions;

/// Default MVT tile extent for the contour tiles we encode.
pub const DEFAULT_MVT_EXTENT: u32 = 4096;

const FEET_TO_METERS: f32 = 0.3048;

/// Units elevations are reported in on the output features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ElevationUnits {
    /// Report elevations in meters (default).
    #[default]
    Meters,
    /// Report elevations in feet.
    Feet,
}

impl ElevationUnits {
    /// Converts `value`, read in these units, into meters.
    fn to_meters(self, value: f32) -> f32 {
        match self {
            Self::Meters => value,
            Self::Feet => value * FEET_TO_METERS,
        }
    }

    /// Converts `meters` into the whole units reported on a feature.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a rounded elevation is inside i16 for every real height, and the cast saturates"
    )]
    fn report(self, meters: f32) -> i16 {
        let value = match self {
            Self::Meters => meters,
            Self::Feet => meters / FEET_TO_METERS,
        };
        value.round() as i16
    }
}

/// Which contour interval to use at different zoom levels.
///
/// Intervals are stored in meters, converted from whatever units they were
/// declared in. An interval of `0` disables contours at that zoom and above,
/// until the next entry.
#[derive(Clone, Debug, PartialEq)]
pub struct ZoomIntervalMap {
    /// `(start zoom, interval in meters)` pairs, sorted by zoom.
    intervals: Vec<(u8, f32)>,
}

impl ZoomIntervalMap {
    /// Builds a map from `(zoom, interval)` pairs, reading each interval in `units`.
    #[must_use]
    pub fn new(intervals: &[(u8, f32)], units: ElevationUnits) -> Self {
        let mut intervals: Vec<(u8, f32)> = intervals
            .iter()
            .map(|&(zoom, value)| (zoom, units.to_meters(value)))
            .collect();
        intervals.sort_by_key(|(zoom, _)| *zoom);
        Self { intervals }
    }

    /// The default interval map for the given units.
    #[must_use]
    pub fn default_for_units(units: ElevationUnits) -> Self {
        match units {
            ElevationUnits::Meters => Self::new(
                &[
                    (0, 0.0),
                    (4, 400.0),
                    (6, 200.0),
                    (8, 150.0),
                    (10, 100.0),
                    (12, 50.0),
                ],
                units,
            ),
            ElevationUnits::Feet => Self::new(
                &[
                    (0, 0.0),
                    (5, 1000.0),
                    (7, 500.0),
                    (9, 400.0),
                    (11, 250.0),
                    (13, 100.0),
                ],
                units,
            ),
        }
    }

    /// The interval in meters that applies at `zoom`.
    #[must_use]
    pub fn interval_meters(&self, zoom: u8) -> f32 {
        self.intervals
            .iter()
            .rfind(|(start, _)| zoom >= *start)
            .map_or(0.0, |(_, interval)| *interval)
    }
}

impl Default for ZoomIntervalMap {
    fn default() -> Self {
        Self::default_for_units(ElevationUnits::default())
    }
}

/// Options controlling isoline tracing.
#[derive(Debug, Clone, PartialEq)]
pub struct IsolineOptions {
    /// The higher the value, the smoother the generated contours.
    pub resolution: f32,
    /// Strategy for choosing the contour interval at a given zoom.
    pub threshold_intervals: ZoomIntervalMap,
    /// How often a major (bolded) contour line is generated. Set to 0 to disable.
    pub major_interval: u32,
    /// Simplification tolerance for contour lines. Higher means more aggressive
    /// simplification and smaller tiles. Set to 0.0 to disable.
    pub simplification_tolerance: f64,
    /// Minimum length; contour lines shorter than this are dropped.
    pub min_feature_length: f64,
    /// Units for reporting elevation values in the output.
    pub elevation_units: ElevationUnits,
    /// A threshold to skip drawing contours at, used to remove artifacts.
    pub filtered_threshold: Option<f32>,
}

impl Default for IsolineOptions {
    fn default() -> Self {
        let elevation_units = ElevationUnits::default();
        Self {
            resolution: 10.0,
            threshold_intervals: ZoomIntervalMap::default_for_units(elevation_units),
            major_interval: 5,
            simplification_tolerance: 10.0,
            min_feature_length: 10.0,
            elevation_units,
            filtered_threshold: None,
        }
    }
}

/// Flat contour-tile options, as a source resolves them from config.
///
/// The pipeline derives its per-stage option structs from these on demand.
#[derive(Debug, Clone, PartialEq)]
pub struct ContourOptions {
    /// The higher the value, the smoother the generated contours.
    pub resolution: f32,
    /// How often a major (bolded) contour line is generated. Set to 0 to disable.
    pub major_interval: u32,
    /// Strategy for choosing the contour interval at a given zoom.
    pub threshold_intervals: ZoomIntervalMap,
    /// Units for reporting elevation values in the output.
    pub elevation_units: ElevationUnits,
    /// Simplification tolerance for contour lines.
    pub simplification_tolerance: f64,
    /// Minimum length; contour lines shorter than this are dropped.
    pub min_feature_length: f64,
    /// A threshold to skip drawing contours at, used to remove artifacts.
    pub filtered_threshold: Option<f32>,
    /// Apron in source pixels traced beyond the tile edge, then transformed back out.
    pub fetch_margin: u8,
    /// MVT layer the features are written into.
    pub layer_name: String,
    /// MVT tile extent.
    pub extent: u32,
}

impl Default for ContourOptions {
    fn default() -> Self {
        Self {
            extent: DEFAULT_MVT_EXTENT,
            // Changing resolution leaves feature positions unchanged (the geometry
            // transform's scaling and margin cancel it out), but simplification and
            // length-filtering run in resolution-scaled space, so it still shifts
            // which features survive.
            resolution: 10.0,
            fetch_margin: 32,
            major_interval: 5,
            layer_name: "contour".to_owned(),
            threshold_intervals: ZoomIntervalMap::default(),
            simplification_tolerance: 10.0,
            min_feature_length: 50.0,
            elevation_units: ElevationUnits::Meters,
            filtered_threshold: Some(0.0),
        }
    }
}

impl ContourOptions {
    /// Isoline-tracing params for [`generate_contours`].
    #[must_use]
    pub fn isoline(&self) -> IsolineOptions {
        IsolineOptions {
            resolution: self.resolution,
            threshold_intervals: self.threshold_intervals.clone(),
            major_interval: self.major_interval,
            simplification_tolerance: self.simplification_tolerance,
            min_feature_length: self.min_feature_length,
            elevation_units: self.elevation_units,
            filtered_threshold: self.filtered_threshold,
        }
    }

    /// Image-space to MVT-space transform.
    ///
    /// `scaling` maps the 256px source grid onto the MVT extent; `margin` is the
    /// fetch apron in the contour's resolution-scaled units (`fetch_margin`
    /// source pixels times `resolution`).
    #[must_use]
    pub fn geometry_transform(&self) -> GeometryTransform {
        GeometryTransform {
            scaling: f64::from(self.extent) / (256.0 * f64::from(self.resolution)),
            margin: f64::from(self.resolution) * f64::from(self.fetch_margin),
        }
    }

    /// MVT encoding target (layer name and extent).
    #[must_use]
    pub fn mvt_encoding(&self) -> MvtEncodingOptions {
        MvtEncodingOptions {
            layer_name: self.layer_name.clone(),
            extent: self.extent,
        }
    }
}

/// Traces contour lines through `grid`.
///
/// The contour interval, and thus the thresholds, is chosen from `opts.threshold_intervals` at `zoom`.
/// Lines are flagged `major` so a style can bold every n-th one.
///
/// # Errors
///
/// Returns [`ContourError::Isolines`] when marching squares rejects the grid.
pub fn generate_contours(
    grid: &HeightGrid,
    zoom: u8,
    opts: &IsolineOptions,
) -> Result<GeometryFeatures, ContourError> {
    let (cols, rows) = grid.dimensions();

    let base_interval_meters = opts.threshold_intervals.interval_meters(zoom);
    if base_interval_meters < 1e-5 {
        return Ok(GeometryFeatures::new(Vec::new()));
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "a major interval is a small count; precision at this magnitude is irrelevant"
    )]
    let major_interval_meters =
        (opts.major_interval > 0).then_some(base_interval_meters * opts.major_interval as f32);

    let thresholds =
        calculate_contour_thresholds(grid, base_interval_meters, opts.filtered_threshold);
    if thresholds.is_empty() {
        return Ok(GeometryFeatures::new(Vec::new()));
    }

    let traced = ContourBuilder::new(cols, rows, true)
        .x_origin(0.0_f32)
        .y_origin(0.0_f32)
        .x_step(opts.resolution)
        .y_step(opts.resolution)
        .lines(grid.values(), &thresholds)
        .map_err(|e| ContourError::Isolines(e.to_string()))?;

    let edge = FieldEdge::new(cols, rows, opts.resolution);
    let mut features = Vec::new();
    for traced_line in &traced {
        let threshold = traced_line.threshold();
        let is_major =
            major_interval_meters.is_some_and(|interval| (threshold % interval).abs() < 1e-5);
        let elevation = opts.elevation_units.report(threshold);
        for ring in traced_line.geometry() {
            let ring = ring.map_coords(|coord| Coord {
                x: f64::from(coord.x),
                y: f64::from(coord.y),
            });
            for run in interior_runs(&ring, edge) {
                if let Some(geometry) = simplify_line(run, opts) {
                    features.push(ContourFeature {
                        geometry,
                        elevation,
                        is_major,
                    });
                }
            }
        }
    }

    Ok(GeometryFeatures::new(features))
}

const EDGE_EPSILON: f64 = 1e-6;

/// The outer edge of the traced field, in the space the rings come back in.
#[derive(Clone, Copy, Debug)]
struct FieldEdge {
    max_x: f64,
    max_y: f64,
}

impl FieldEdge {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a grid side is a few hundred samples, exactly representable"
    )]
    fn new(cols: usize, rows: usize, resolution: f32) -> Self {
        Self {
            max_x: cols as f64 * f64::from(resolution),
            max_y: rows as f64 * f64::from(resolution),
        }
    }

    fn runs_along(self, segment: Line) -> bool {
        let shared = |a: f64, b: f64, far: f64| {
            (a.abs() < EDGE_EPSILON && b.abs() < EDGE_EPSILON)
                || ((a - far).abs() < EDGE_EPSILON && (b - far).abs() < EDGE_EPSILON)
        };
        shared(segment.start.x, segment.end.x, self.max_x)
            || shared(segment.start.y, segment.end.y, self.max_y)
    }
}

/// Splits a closed ring into the runs that do not follow the field edge.
fn interior_runs(ring: &LineString, edge: FieldEdge) -> Vec<LineString> {
    let mut segments: Vec<Line> = ring.lines().collect();
    let Some(first_edge) = segments.iter().position(|s| edge.runs_along(*s)) else {
        return vec![ring.clone()];
    };
    segments.rotate_left(first_edge);
    segments
        .split(|s| edge.runs_along(*s))
        .filter_map(|run| {
            let head = run.first()?;
            Some(
                std::iter::once(head.start)
                    .chain(run.iter().map(|s| s.end))
                    .collect(),
            )
        })
        .collect()
}

/// Calculates the contour thresholds for a grid, dropping any filtered elevation.
fn calculate_contour_thresholds(
    grid: &HeightGrid,
    base_interval_meters: f32,
    filtered_threshold: Option<f32>,
) -> Vec<f32> {
    let mut thresholds = grid.get_thresholds(base_interval_meters);
    if let Some(filter_elevation) = filtered_threshold {
        thresholds.retain(|&t| (t - filter_elevation).abs() > 1e-5);
    }
    thresholds
}

/// Simplifies and length-filters a contour line, returning `None` if the result
/// is too short or has too few points to keep.
fn simplify_line(mut line: LineString, opts: &IsolineOptions) -> Option<LineString> {
    if opts.simplification_tolerance > 0.0 {
        line = line.simplify(opts.simplification_tolerance);
    }

    if line.0.len() < 2 {
        return None;
    }

    let line_length = line
        .0
        .windows(2)
        .map(|pair| {
            let dx = pair[1].x - pair[0].x;
            let dy = pair[1].y - pair[0].y;
            dx.hypot(dy)
        })
        .sum::<f64>();

    if line_length < opts.min_feature_length {
        return None;
    }

    Some(line)
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;

    use super::*;

    /// Pins the per-stage config `ContourOptions` derives from its default flat
    /// params. The scaling/margin derivation has no golden coverage, so this
    /// guards it.
    #[test]
    fn default_derives_expected_config() {
        let opts = ContourOptions::default();

        let transform = opts.geometry_transform();
        assert_relative_eq!(transform.scaling, 1.6); // 4096 / (256 * 10)
        assert_relative_eq!(transform.margin, 320.0); // 10 * 32

        let iso = opts.isoline();
        assert_relative_eq!(iso.resolution, 10.0);
        assert_eq!(iso.major_interval, 5);
        assert_relative_eq!(iso.simplification_tolerance, 10.0);
        assert_relative_eq!(iso.min_feature_length, 50.0);
        assert_eq!(iso.elevation_units, ElevationUnits::Meters);
        assert_relative_eq!(iso.filtered_threshold.expect("default filters 0"), 0.0);

        assert_eq!(opts.mvt_encoding().extent, 4096);
        assert_eq!(opts.mvt_encoding().layer_name.as_str(), "contour");
        assert_eq!(opts.fetch_margin, 32);
    }

    /// Overriding the flat params recomputes the derived scaling and margin.
    #[test]
    fn overrides_recompute_scaling_and_margin() {
        let opts = ContourOptions {
            resolution: 8.0,
            fetch_margin: 16,
            extent: 2048,
            ..Default::default()
        };

        let transform = opts.geometry_transform();
        assert_relative_eq!(transform.scaling, 1.0); // 2048 / (256 * 8)
        assert_relative_eq!(transform.margin, 128.0); // 8 * 16
        assert_eq!(opts.mvt_encoding().extent, 2048);
        assert_eq!(opts.fetch_margin, 16);
    }

    #[test]
    fn zoom_interval_map_converts_declared_units_to_meters() {
        let intervals = [(0, 0.0), (5, 100.0), (10, 50.0)];

        let map_meters = ZoomIntervalMap::new(&intervals, ElevationUnits::Meters);
        assert_relative_eq!(map_meters.interval_meters(8), 100.0);

        let map_feet = ZoomIntervalMap::new(&intervals, ElevationUnits::Feet);
        assert_relative_eq!(map_feet.interval_meters(12), 15.24, epsilon = 0.01);
    }

    #[test]
    fn zoom_below_the_first_entry_disables_contours() {
        let map = ZoomIntervalMap::new(&[(5, 100.0)], ElevationUnits::Meters);
        assert_relative_eq!(map.interval_meters(4), 0.0);
        assert_relative_eq!(map.interval_meters(5), 100.0);
    }

    #[test]
    fn a_zoom_with_a_zero_interval_traces_nothing() {
        let grid = HeightGrid::from_values(vec![0.0, 100.0, 200.0, 300.0], 2, 2);
        let features = generate_contours(&grid, 0, &IsolineOptions::default())
            .expect("a zero interval is not an error");
        assert!(features.is_empty());
    }

    #[test]
    fn major_lines_are_every_nth_multiple_of_the_interval() {
        let opts = IsolineOptions {
            threshold_intervals: ZoomIntervalMap::new(&[(0, 100.0)], ElevationUnits::Meters),
            major_interval: 5,
            simplification_tolerance: 0.0,
            min_feature_length: 0.0,
            filtered_threshold: None,
            ..Default::default()
        };
        let side = 16;
        let values: Vec<f32> = (0..side * side)
            .map(|i| {
                #[expect(clippy::cast_precision_loss, reason = "a small test index")]
                let row = (i / side) as f32;
                row * 100.0
            })
            .collect();
        let grid = HeightGrid::from_values(values, side, side);

        let features = generate_contours(&grid, 0, &opts).expect("a ramp traces");
        let major_elevations: Vec<i16> = features
            .iter()
            .filter(|f| f.is_major)
            .map(|f| f.elevation)
            .collect();

        assert!(!major_elevations.is_empty(), "the ramp crosses 0 and 500");
        for elevation in major_elevations {
            assert_eq!(elevation % 500, 0);
        }
    }

    #[test]
    fn filtered_threshold_is_dropped() {
        let grid = HeightGrid::from_values(vec![-100.0, 100.0, -100.0, 100.0], 2, 2);
        let with_filter = calculate_contour_thresholds(&grid, 100.0, Some(0.0));
        let without_filter = calculate_contour_thresholds(&grid, 100.0, None);

        assert!(without_filter.iter().any(|t| t.abs() < 1e-5));
        assert!(!with_filter.iter().any(|t| t.abs() < 1e-5));
    }

    #[test]
    fn elevation_is_reported_in_the_configured_units() {
        assert_eq!(ElevationUnits::Meters.report(100.0), 100);
        assert_eq!(ElevationUnits::Feet.report(15.24), 50);
    }
}

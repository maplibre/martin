//! Configuration for the contour post-cache processor.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::RangeInclusive;

use martin_core::tiles::contour::{ContourOptions, ElevationUnits, ZoomIntervalMap};
use serde::de::{self, Unexpected, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::config::file::{CollectUnrecognizedKeys, UnrecognizedKeys, UnrecognizedValues};
use crate::config::primitives::AutoOption;

/// Largest accepted [`ContourSettings::extent`].
///
/// The extent is the integer grid a tile's coordinates are quantised onto.
/// Past this it stops buying precision and only inflates every varint.
pub const MAX_EXTENT: u32 = 16_384;

/// Largest accepted [`ContourSettings::fetch_margin`], in source pixels.
///
/// The margin is traced beyond the tile edge and transformed back out, so it is
/// pure overdraw: past a small fraction of the 256px source tile it costs
/// tracing time without joining any more lines across the seam.
pub const MAX_FETCH_MARGIN: u32 = 64;

/// A contour parameter outside the range it is defined over.
#[derive(thiserror::Error, Debug, PartialEq, Eq, Clone)]
#[error("Contour parameter {name} must be between `{low}` and `{high}`, but was `{value}`")]
pub struct ContourRangeError {
    /// Name of the parameter, as spelled in config and in the query.
    pub name: String,
    /// Value that was rejected, rendered for display.
    pub value: String,
    /// Inclusive lower bound.
    pub low: String,
    /// Inclusive upper bound.
    pub high: String,
}

/// Three-state contour setting, matching the other processors:
///
/// - `auto` / `default` / `true` - trace with every default
/// - `disabled` / `off` / `no` / `false` - do not trace
/// - a map of settings - trace, overriding the named defaults
///
/// Settable per source only, never globally or per source type.
/// A global default would leak into vector and normal-map sources, which carry
/// no elevation to trace at all.
pub type ContourProcessConfig = AutoOption<ContourSettings>;

/// Units elevations are reported in on the output features.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub enum ContourElevationUnits {
    /// Report elevations in meters.
    #[default]
    #[serde(rename = "meters")]
    Meters,
    /// Report elevations in feet.
    #[serde(rename = "feet")]
    Feet,
}

impl From<ContourElevationUnits> for ElevationUnits {
    fn from(value: ContourElevationUnits) -> Self {
        match value {
            ContourElevationUnits::Meters => Self::Meters,
            ContourElevationUnits::Feet => Self::Feet,
        }
    }
}

/// An elevation whose contour line is suppressed, or no suppression at all.
///
/// Defaults to sea level: a flat ocean tile otherwise answers with a contour
/// ring tracing its own edge, since the whole grid sits on one threshold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FilteredThreshold {
    /// Draw every threshold, including sea level.
    Disabled,
    /// Skip the contour at this elevation, in the configured units.
    Elevation(f32),
}

impl Default for FilteredThreshold {
    fn default() -> Self {
        Self::Elevation(0.0)
    }
}

impl FilteredThreshold {
    /// The elevation to skip, or `None` when nothing is filtered.
    #[must_use]
    pub fn as_elevation(self) -> Option<f32> {
        match self {
            Self::Disabled => None,
            Self::Elevation(v) => Some(v),
        }
    }
}

impl Serialize for FilteredThreshold {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Disabled => serializer.serialize_str("disabled"),
            Self::Elevation(v) => serializer.serialize_f32(*v),
        }
    }
}

impl<'de> Deserialize<'de> for FilteredThreshold {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(FilteredThresholdVisitor)
    }
}

struct FilteredThresholdVisitor;

impl Visitor<'_> for FilteredThresholdVisitor {
    type Value = FilteredThreshold;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(r#"an elevation number or the string "disabled""#)
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        match v {
            "disabled" | "off" | "none" => Ok(FilteredThreshold::Disabled),
            _ => Err(E::invalid_value(Unexpected::Str(v), &self)),
        }
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
        if v {
            Err(E::invalid_value(Unexpected::Bool(v), &self))
        } else {
            Ok(FilteredThreshold::Disabled)
        }
    }

    #[expect(clippy::cast_possible_truncation, reason = "an elevation in meters")]
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
        Ok(FilteredThreshold::Elevation(v as f32))
    }

    #[expect(clippy::cast_precision_loss, reason = "an elevation in meters")]
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
        Ok(FilteredThreshold::Elevation(v as f32))
    }

    #[expect(clippy::cast_precision_loss, reason = "an elevation in meters")]
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
        Ok(FilteredThreshold::Elevation(v as f32))
    }
}

#[cfg(feature = "unstable-schemas")]
impl schemars::JsonSchema for FilteredThreshold {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("FilteredThreshold")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description":
                "An elevation whose contour line is suppressed:\n\n\
                 - A number - skip the contour at this elevation\n\
                 - `\"disabled\"` or boolean `false` - draw every threshold",
            "oneOf": [
                { "type": "number", "description": "Elevation to skip." },
                {
                    "type": "string",
                    "enum": ["disabled", "off", "none"],
                    "description": "Draw every threshold."
                },
                { "type": "boolean", "description": "false = draw every threshold." },
            ]
        })
    }
}

/// The cartographic zoom-to-interval ramp `ContourSettings::zoom_intervals` falls back to.
#[cfg(feature = "unstable-schemas")]
fn zoom_intervals_example() -> BTreeMap<u8, f32> {
    BTreeMap::from([
        (0, 0.0),
        (4, 400.0),
        (6, 200.0),
        (8, 150.0),
        (10, 100.0),
        (12, 50.0),
    ])
}

/// Contour settings as they appear in a config file.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct ContourSettings {
    /// Sampling step the isolines are traced at; higher is smoother.
    /// Defaults to `10`.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &10.0f64))]
    pub resolution: Option<f64>,
    /// How often a major (bolded) contour line is generated. `0` disables them.
    /// Defaults to `5`.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &5.0f64))]
    pub major_interval: Option<f64>,
    /// Douglas-Peucker tolerance. Higher means smaller tiles and coarser lines;
    /// `0` disables simplification.
    /// Defaults to `10`.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &10.0f64))]
    pub simplification_tolerance: Option<f64>,
    /// Contour lines shorter than this are dropped.
    /// Defaults to `50`.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &50.0f64))]
    pub min_feature_length: Option<f64>,
    /// Apron in source pixels traced past the tile edge so a line meets its
    /// continuation in the next tile, then transformed back out.
    /// Defaults to `32`.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &32.0f64))]
    pub fetch_margin: Option<f64>,
    /// Contour interval per zoom level, as `zoom: interval` in
    /// [`Self::elevation_units`]. An interval of `0` disables contours from that
    /// zoom until the next entry.
    /// Defaults to a cartographic ramp from `400` at z4 down to `50` at z12.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = zoom_intervals_example()))]
    pub zoom_intervals: Option<BTreeMap<u8, f32>>,
    /// Units elevations are declared and reported in.
    /// Defaults to `meters`.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"meters"))]
    pub elevation_units: Option<ContourElevationUnits>,
    /// An elevation whose contour is suppressed, or `disabled` to draw them all.
    /// Defaults to `0` (sea level).
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &0.0f32))]
    pub filtered_threshold: Option<FilteredThreshold>,
    /// MVT layer the contour features are written into.
    /// Defaults to `contour`.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"contour"))]
    pub layer_name: Option<String>,
    /// MVT tile extent.
    /// Defaults to `4096`.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &4096u32))]
    pub extent: Option<u32>,
    /// Whether a request may override these settings with query parameters.
    /// Defaults to `false`.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &false))]
    pub allow_request_overrides: Option<bool>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

/// Only the flattened catch-all can hold unrecognized keys; every other field is
/// a scalar or a map whose own keys are zoom levels rather than setting names.
impl CollectUnrecognizedKeys for ContourSettings {
    fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys) {
        CollectUnrecognizedKeys::collect_unrecognized(&self.unrecognized, path, out);
    }
}

/// Contour settings after validation, in the form the tracer consumes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResolvedContour {
    /// Tracing and encoding parameters.
    pub opts: ContourOptions,
    /// Whether query parameters may override the above.
    pub allow_request_overrides: bool,
}

struct NumericParam {
    name: &'static str,
    range: RangeInclusive<f64>,
    configured: fn(&ContourSettings) -> Option<f64>,
    apply: fn(&mut ContourOptions, f64),
    current: fn(&ContourOptions) -> f64,
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "every value is range-checked into its target type before the cast"
)]
static NUMERIC_PARAMS: [NumericParam; 5] = [
    NumericParam {
        name: "resolution",
        range: 1.0..=64.0,
        configured: |settings| settings.resolution,
        apply: |opts, v| opts.resolution = v as f32,
        current: |opts| f64::from(opts.resolution),
    },
    NumericParam {
        name: "major_interval",
        range: 0.0..=64.0,
        configured: |settings| settings.major_interval,
        apply: |opts, v| opts.major_interval = v as u32,
        current: |opts| f64::from(opts.major_interval),
    },
    NumericParam {
        name: "simplification_tolerance",
        range: 0.0..=100.0,
        configured: |settings| settings.simplification_tolerance,
        apply: |opts, v| opts.simplification_tolerance = v,
        current: |opts| opts.simplification_tolerance,
    },
    NumericParam {
        name: "min_feature_length",
        range: 0.0..=10_000.0,
        configured: |settings| settings.min_feature_length,
        apply: |opts, v| opts.min_feature_length = v,
        current: |opts| opts.min_feature_length,
    },
    NumericParam {
        name: "fetch_margin",
        range: 0.0..=MAX_FETCH_MARGIN as f64,
        configured: |settings| settings.fetch_margin,
        apply: |opts, v| opts.fetch_margin = v as u8,
        current: |opts| f64::from(opts.fetch_margin),
    },
];

impl NumericParam {
    /// Returns `value` if in range, else an error naming the parameter.
    ///
    /// `RangeInclusive::contains` is false for NaN and infinity, so those are
    /// rejected without a separate check.
    fn checked(&self, value: f64) -> Result<f64, ContourRangeError> {
        if self.range.contains(&value) {
            Ok(value)
        } else {
            Err(self.rejected(value.to_string()))
        }
    }

    fn rejected(&self, value: String) -> ContourRangeError {
        ContourRangeError {
            name: self.name.to_owned(),
            value,
            low: self.range.start().to_string(),
            high: self.range.end().to_string(),
        }
    }
}

impl ContourSettings {
    /// Validates these settings and resolves them against the defaults.
    ///
    /// # Errors
    ///
    /// Returns [`ContourRangeError`] when a parameter is outside its range.
    #[expect(
        clippy::unneeded_field_pattern,
        reason = "the ignored fields are resolved through NUMERIC_PARAMS; naming them \
                  anyway means adding a field without listing it there is a compile \
                  error, which `..` would hide"
    )]
    pub fn resolve(&self) -> Result<ResolvedContour, ContourRangeError> {
        let Self {
            resolution: _,
            major_interval: _,
            simplification_tolerance: _,
            min_feature_length: _,
            fetch_margin: _,
            zoom_intervals,
            elevation_units,
            filtered_threshold,
            layer_name,
            extent,
            allow_request_overrides,
            unrecognized: _,
        } = self;

        let mut out = ResolvedContour::default();

        for param in &NUMERIC_PARAMS {
            if let Some(v) = (param.configured)(self) {
                (param.apply)(&mut out.opts, param.checked(v)?);
            }
        }

        // Units decide how the intervals and the filtered threshold are read, so
        // they are resolved before either.
        let units = elevation_units.unwrap_or_default().into();
        out.opts.elevation_units = units;
        out.opts.threshold_intervals = match zoom_intervals {
            Some(map) => {
                let pairs: Vec<(u8, f32)> = map.iter().map(|(&z, &i)| (z, i)).collect();
                ZoomIntervalMap::new(&pairs, units)
            }
            None => ZoomIntervalMap::default_for_units(units),
        };
        out.opts.filtered_threshold = filtered_threshold.unwrap_or_default().as_elevation();

        if let Some(name) = layer_name {
            if name.is_empty() {
                return Err(ContourRangeError {
                    name: "layer_name".to_owned(),
                    value: String::new(),
                    low: "1".to_owned(),
                    high: "any".to_owned(),
                });
            }
            out.opts.layer_name.clone_from(name);
        }
        if let Some(v) = *extent {
            if v == 0 || v > MAX_EXTENT {
                return Err(ContourRangeError {
                    name: "extent".to_owned(),
                    value: v.to_string(),
                    low: "1".to_owned(),
                    high: MAX_EXTENT.to_string(),
                });
            }
            out.opts.extent = v;
        }
        if let Some(v) = *allow_request_overrides {
            out.allow_request_overrides = v;
        }

        Ok(out)
    }
}

impl ContourProcessConfig {
    /// Whether contouring is switched on, without resolving the settings.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        !matches!(self, Self::Disabled)
    }

    /// Resolves this setting into tracer parameters, or `None` when contouring is disabled.
    ///
    /// # Errors
    ///
    /// Returns [`ContourRangeError`] when a parameter is outside its range.
    pub fn resolve_contour(&self) -> Result<Option<ResolvedContour>, ContourRangeError> {
        match self {
            Self::Disabled => Ok(None),
            Self::Auto => Ok(Some(ResolvedContour::default())),
            Self::Explicit(settings) => settings.resolve().map(Some),
        }
    }
}

impl ResolvedContour {
    /// Applies query-parameter overrides on top of these settings.
    ///
    /// Each parameter is a plain decimal number under the same name it has in
    /// config. Unrecognized keys are ignored rather than rejected, since the
    /// query string is shared with cache key and source selection.
    ///
    /// Returns the settings unchanged when [`Self::allow_request_overrides`] is false.
    ///
    /// # Errors
    ///
    /// Returns [`ContourRangeError`] when a supplied value does not parse or is
    /// outside its range.
    pub fn with_query_overrides<'a, I>(mut self, params: I) -> Result<Self, ContourRangeError>
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        if !self.allow_request_overrides {
            return Ok(self);
        }
        for (key, raw) in params {
            let Some(param) = NUMERIC_PARAMS.iter().find(|param| param.name == key) else {
                continue;
            };
            let value = raw
                .parse::<f64>()
                .map_err(|_e| param.rejected(raw.to_owned()))?;
            (param.apply)(&mut self.opts, param.checked(value)?);
        }
        Ok(self)
    }

    /// A short, stable fingerprint of everything that affects the encoded bytes.
    ///
    /// Used as part of the tile's etag; must change whenever any parameter
    /// does, or a client keeps a stale tile after a re-tune.
    #[must_use]
    #[expect(
        clippy::unneeded_field_pattern,
        reason = "the ignored fields are folded in through NUMERIC_PARAMS; naming \
                  every field anyway means adding one that affects the encoded bytes \
                  without folding it in here is a compile error, which `..` would hide"
    )]
    pub fn fingerprint(&self) -> String {
        let ContourOptions {
            resolution: _,
            major_interval: _,
            simplification_tolerance: _,
            min_feature_length: _,
            fetch_margin: _,
            threshold_intervals,
            elevation_units,
            filtered_threshold,
            layer_name,
            extent,
        } = &self.opts;

        let numeric = NUMERIC_PARAMS
            .iter()
            .map(|param| (param.current)(&self.opts).to_string())
            .collect::<Vec<_>>()
            .join(":");
        let intervals = (0..=24u8)
            .map(|z| threshold_intervals.interval_meters(z).to_string())
            .collect::<Vec<_>>()
            .join(",");
        let filtered = filtered_threshold.map_or_else(|| "off".to_owned(), |v| v.to_string());
        format!("{numeric}:{intervals}:{elevation_units:?}:{filtered}:{layer_name}:{extent}")
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    /// The exact YAML shape the end-to-end config uses.
    #[test]
    fn nested_override_settings_parse_from_a_source_block() {
        let cfg: ContourProcessConfig = serde_saphyr::from_str(indoc! {"
            allow_request_overrides: true
            simplification_tolerance: 0
        "})
        .unwrap();
        let resolved = cfg.resolve_contour().unwrap().unwrap();
        assert!(resolved.allow_request_overrides);
        assert!((resolved.opts.simplification_tolerance - 0.0).abs() < 1e-9);

        let overridden = resolved
            .with_query_overrides([("simplification_tolerance", "50")])
            .unwrap();
        assert!((overridden.opts.simplification_tolerance - 50.0).abs() < 1e-9);
    }

    #[test]
    fn parse_auto_string() {
        let cfg: ContourProcessConfig = serde_saphyr::from_str("auto").unwrap();
        assert_eq!(cfg, ContourProcessConfig::Auto);
        assert_eq!(cfg.resolve_contour().unwrap(), Some(ResolvedContour::default()));
    }

    #[test]
    fn parse_disabled_string() {
        let cfg: ContourProcessConfig = serde_saphyr::from_str("disabled").unwrap();
        assert_eq!(cfg.resolve_contour().unwrap(), None);
    }

    #[test]
    fn explicit_settings_override_only_what_they_name() {
        let cfg: ContourProcessConfig = serde_saphyr::from_str(indoc! {"
            resolution: 8
            min_feature_length: 25
        "})
        .unwrap();
        let resolved = cfg.resolve_contour().unwrap().unwrap();

        assert!((resolved.opts.resolution - 8.0).abs() < 1e-6);
        assert!((resolved.opts.min_feature_length - 25.0).abs() < 1e-6);
        assert_eq!(resolved.opts.major_interval, ContourOptions::default().major_interval);
        assert_eq!(resolved.opts.layer_name, "contour");
        assert_eq!(resolved.opts.extent, 4096);
    }

    #[test]
    fn zoom_intervals_are_read_in_the_configured_units() {
        let cfg: ContourProcessConfig = serde_saphyr::from_str(indoc! {"
            elevation_units: feet
            zoom_intervals:
              0: 0
              10: 50
        "})
        .unwrap();
        let resolved = cfg.resolve_contour().unwrap().unwrap();

        // 50 ft is 15.24 m.
        assert!((resolved.opts.threshold_intervals.interval_meters(10) - 15.24).abs() < 0.01);
        assert_eq!(resolved.opts.elevation_units, ElevationUnits::Feet);
    }

    #[test]
    fn filtered_threshold_defaults_to_sea_level_and_can_be_switched_off() {
        let default: ContourProcessConfig = serde_saphyr::from_str("{}").unwrap();
        assert_eq!(
            default
                .resolve_contour()
                .unwrap()
                .unwrap()
                .opts
                .filtered_threshold,
            Some(0.0)
        );

        let off: ContourProcessConfig =
            serde_saphyr::from_str("filtered_threshold: disabled").unwrap();
        assert_eq!(
            off.resolve_contour()
                .unwrap()
                .unwrap()
                .opts
                .filtered_threshold,
            None
        );

        let explicit: ContourProcessConfig =
            serde_saphyr::from_str("filtered_threshold: -50").unwrap();
        assert_eq!(
            explicit
                .resolve_contour()
                .unwrap()
                .unwrap()
                .opts
                .filtered_threshold,
            Some(-50.0)
        );
    }

    #[test]
    fn out_of_range_parameters_are_rejected() {
        let cfg: ContourProcessConfig = serde_saphyr::from_str("resolution: 0").unwrap();
        let err = cfg.resolve_contour().unwrap_err();
        assert_eq!(err.name, "resolution");

        let cfg: ContourProcessConfig = serde_saphyr::from_str("fetch_margin: 500").unwrap();
        assert_eq!(cfg.resolve_contour().unwrap_err().name, "fetch_margin");

        let cfg: ContourProcessConfig = serde_saphyr::from_str("extent: 0").unwrap();
        assert_eq!(cfg.resolve_contour().unwrap_err().name, "extent");

        let cfg: ContourProcessConfig = serde_saphyr::from_str("layer_name: ''").unwrap();
        assert_eq!(cfg.resolve_contour().unwrap_err().name, "layer_name");
    }

    #[test]
    fn query_overrides_are_ignored_unless_opted_in() {
        let locked = ResolvedContour::default();
        let unchanged = locked
            .clone()
            .with_query_overrides([("resolution", "32")])
            .unwrap();
        assert_eq!(unchanged, locked);

        let open = ResolvedContour {
            allow_request_overrides: true,
            ..Default::default()
        };
        let overridden = open.with_query_overrides([("resolution", "32")]).unwrap();
        assert!((overridden.opts.resolution - 32.0).abs() < 1e-6);
    }

    #[test]
    fn query_overrides_are_range_checked_and_unknown_keys_ignored() {
        let open = ResolvedContour {
            allow_request_overrides: true,
            ..Default::default()
        };
        assert_eq!(
            open.clone()
                .with_query_overrides([("resolution", "999")])
                .unwrap_err()
                .name,
            "resolution"
        );
        assert_eq!(
            open.clone()
                .with_query_overrides([("resolution", "not-a-number")])
                .unwrap_err()
                .name,
            "resolution"
        );
        assert_eq!(
            open.clone()
                .with_query_overrides([("totally_unrelated", "1")])
                .unwrap(),
            open
        );
    }

    #[test]
    fn the_fingerprint_changes_with_every_setting_that_changes_the_bytes() {
        let base = ResolvedContour::default();
        let baseline = base.fingerprint();

        let mut resolution = base.clone();
        resolution.opts.resolution = 12.0;
        assert_ne!(resolution.fingerprint(), baseline);

        let mut layer = base.clone();
        layer.opts.layer_name = "other".to_owned();
        assert_ne!(layer.fingerprint(), baseline);

        let mut intervals = base.clone();
        intervals.opts.threshold_intervals =
            ZoomIntervalMap::new(&[(0, 25.0)], ElevationUnits::Meters);
        assert_ne!(intervals.fingerprint(), baseline);

        let mut filtered = base.clone();
        filtered.opts.filtered_threshold = None;
        assert_ne!(filtered.fingerprint(), baseline);

        // Permission itself does not change the bytes, only the values it lets through.
        let mut overrides = base.clone();
        overrides.allow_request_overrides = true;
        assert_eq!(overrides.fingerprint(), baseline);
    }

    #[test]
    fn unrecognized_keys_are_collected() {
        let cfg: ContourProcessConfig = serde_saphyr::from_str(indoc! {"
            resolution: 8
            definitely_a_typo: 1
        "})
        .unwrap();
        let ContourProcessConfig::Explicit(inner) = cfg else {
            panic!("expected explicit ContourSettings");
        };
        assert!(inner.get_unrecognized_keys().contains("definitely_a_typo"));
    }
}

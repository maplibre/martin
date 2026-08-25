//! Configuration for the hillshade post-cache processor.

use std::ops::RangeInclusive;

use martin_core::tiles::hillshade::{BakeParams, LightAngles};
use martin_tile_utils::Format;
use serde::{Deserialize, Serialize};

use crate::config::file::{CollectUnrecognizedKeys, UnrecognizedValues};
use crate::config::primitives::AutoOption;

/// Largest accepted [`HillshadeSettings::padding`], in pixels.
///
/// Padding is overdraw for edge sampling
/// Beyond this cap it stops being overdraw and becomes a differently-sized tile.
pub const MAX_PADDING: u32 = 32;

/// A hillshade parameter outside the range it is defined over.
#[derive(thiserror::Error, Debug, PartialEq, Eq, Clone)]
#[error("Hillshade parameter {name} must be between `{low}` and `{high}`, but was `{value}`")]
pub struct HillshadeRangeError {
    /// Name of the parameter, as spelled in config and in the query.
    pub name: String,
    /// Value that was rejected, rendered for display.
    pub value: String,
    /// Inclusive lower bound.
    pub low: String,
    /// Inclusive upper bound.
    pub high: String,
}

/// Three-state hillshade setting, matching the other processors:
///
/// - `auto` / `default` / `true` - bake with every default
/// - `disabled` / `off` / `no` / `false` - do not bake
/// - a map of settings - bake, overriding the named defaults
///
/// Settable per source only, never globally or per source type.
/// A global default would leak into vector and elevation sources that cannot be shaded at all/currently
pub type HillshadeProcessConfig = AutoOption<HillshadeSettings>;

/// Image format for the baked hillshade.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, CollectUnrecognizedKeys,
)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub enum HillshadeFormat {
    /// Lossless PNG. Universally supported.
    #[default]
    #[serde(rename = "png")]
    Png,
    /// Lossless WebP. Around 40% smaller than PNG on banded output.
    #[serde(rename = "webp")]
    Webp,
}

impl From<HillshadeFormat> for Format {
    fn from(value: HillshadeFormat) -> Self {
        match value {
            HillshadeFormat::Png => Self::Png,
            HillshadeFormat::Webp => Self::Webp,
        }
    }
}

/// Hillshade settings as they appear in a config file.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct HillshadeSettings {
    /// Compass bearing the light shines from, in degrees clockwise from north.
    /// Defaults to `315` (north-west) by cartographic convention.
    pub azimuth: Option<f64>,
    /// Height of the light above the horizon in degrees.
    /// Defaults to `45`.
    pub altitude: Option<f64>,
    /// Scales the terrain's horizontal gradient before lighting, exaggerating relief.
    /// `1` is true-to-source.
    /// Defaults to `3`.
    pub vertical_exaggeration: Option<f64>,
    /// Separation between lit and shadowed slopes.
    /// Defaults to `1.9`.
    pub contrast: Option<f64>,
    /// How strongly high terrain deepens the contrast.
    /// Defaults to `2.5`.
    pub elevation_scale: Option<f64>,
    /// Number of hard shading bands; below `2` the shading is a smooth gradient instead.
    /// Defaults to `3`.
    pub toon_bands: Option<f64>,
    /// Shadow floor, so shadows read as shaded rather than black.
    /// Defaults to `0.25`.
    pub ambient: Option<f64>,
    /// Apron width in pixels at 256-core scale, rescaled with the core.
    /// Defaults to `0`, so the served tile is a 512x512 square.
    pub padding: Option<u32>,
    /// Output image format.
    /// Defaults to `png`.
    pub format: Option<HillshadeFormat>,
    /// Whether a request may override these settings with query parameters.
    /// Defaults to `false`.
    pub allow_request_overrides: Option<bool>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

/// Hillshade settings after validation, in the form the kernel consumes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedHillshade {
    /// Direction the terrain is lit from.
    pub light: LightAngles,
    /// Kernel parameters.
    pub bake: BakeParams,
    /// Output image format.
    pub format: Format,
    /// Whether query parameters may override the above.
    pub allow_request_overrides: bool,
}

impl Default for ResolvedHillshade {
    fn default() -> Self {
        Self {
            light: LightAngles::default(),
            bake: BakeParams::default(),
            format: Format::Png,
            allow_request_overrides: false,
        }
    }
}

struct NumericParam {
    name: &'static str,
    range: RangeInclusive<f64>,
    configured: fn(&HillshadeSettings) -> Option<f64>,
    slot: fn(&mut ResolvedHillshade) -> &mut f64,
}

static NUMERIC_PARAMS: [NumericParam; 7] = [
    NumericParam {
        name: "azimuth",
        range: 0.0..=360.0,
        configured: |settings| settings.azimuth,
        slot: |resolved| &mut resolved.light.azimuth_deg,
    },
    NumericParam {
        name: "altitude",
        range: 0.0..=90.0,
        configured: |settings| settings.altitude,
        slot: |resolved| &mut resolved.light.altitude_deg,
    },
    NumericParam {
        name: "vertical_exaggeration",
        range: 0.0..=10.0,
        configured: |settings| settings.vertical_exaggeration,
        slot: |resolved| &mut resolved.bake.vertical_exaggeration,
    },
    NumericParam {
        name: "contrast",
        range: 0.0..=10.0,
        configured: |settings| settings.contrast,
        slot: |resolved| &mut resolved.bake.contrast,
    },
    NumericParam {
        name: "elevation_scale",
        range: 0.0..=10.0,
        configured: |settings| settings.elevation_scale,
        slot: |resolved| &mut resolved.bake.elevation_scale,
    },
    NumericParam {
        name: "toon_bands",
        range: 0.0..=32.0,
        configured: |settings| settings.toon_bands,
        slot: |resolved| &mut resolved.bake.toon_bands,
    },
    NumericParam {
        name: "ambient",
        range: 0.0..=1.0,
        configured: |settings| settings.ambient,
        slot: |resolved| &mut resolved.bake.ambient,
    },
];

impl NumericParam {
    /// Returns `value` if in range, else an error naming the parameter.
    ///
    /// `RangeInclusive::contains` is false for NaN and infinity, so those are
    /// rejected without a separate check.
    fn checked(&self, value: f64) -> Result<f64, HillshadeRangeError> {
        if self.range.contains(&value) {
            Ok(value)
        } else {
            Err(self.rejected(value.to_string()))
        }
    }

    fn rejected(&self, value: String) -> HillshadeRangeError {
        HillshadeRangeError {
            name: self.name.to_owned(),
            value,
            low: self.range.start().to_string(),
            high: self.range.end().to_string(),
        }
    }
}

impl HillshadeSettings {
    /// Validates these settings and resolves them against the defaults.
    #[expect(
        clippy::unneeded_field_pattern,
        reason = "the ignored fields are resolved through NUMERIC_PARAMS; naming them \
                  anyway means adding a field without listing it there is a compile \
                  error, which `..` would hide"
    )]
    pub fn resolve(&self) -> Result<ResolvedHillshade, HillshadeRangeError> {
        let Self {
            azimuth: _,
            altitude: _,
            vertical_exaggeration: _,
            contrast: _,
            elevation_scale: _,
            toon_bands: _,
            ambient: _,
            padding,
            format,
            allow_request_overrides,
            unrecognized: _,
        } = self;

        let mut out = ResolvedHillshade::default();

        for param in &NUMERIC_PARAMS {
            if let Some(v) = (param.configured)(self) {
                *(param.slot)(&mut out) = param.checked(v)?;
            }
        }
        if let Some(v) = *padding {
            if v > MAX_PADDING {
                return Err(HillshadeRangeError {
                    name: "padding".to_owned(),
                    value: v.to_string(),
                    low: "0".to_owned(),
                    high: MAX_PADDING.to_string(),
                });
            }
            out.bake.padding = v;
        }
        if let Some(v) = *format {
            out.format = v.into();
        }
        if let Some(v) = *allow_request_overrides {
            out.allow_request_overrides = v;
        }

        Ok(out)
    }
}

impl HillshadeProcessConfig {
    /// Resolves this setting into kernel parameters, or `None` when hillshading is disabled.
    pub fn resolve_hillshade(&self) -> Result<Option<ResolvedHillshade>, HillshadeRangeError> {
        match self {
            Self::Disabled => Ok(None),
            Self::Auto => Ok(Some(ResolvedHillshade::default())),
            Self::Explicit(settings) => settings.resolve().map(Some),
        }
    }
}

impl ResolvedHillshade {
    /// Applies query-parameter overrides on top of these settings.
    ///
    /// Each parameter is a plain decimal number under the same name it has in config.
    /// Unrecognized keys are ignored rather than rejected, since the query string is shared with cache key and source selection.
    ///
    /// Returns the settings unchanged when [`Self::allow_request_overrides`] is false.
    pub fn with_query_overrides<'a, I>(mut self, params: I) -> Result<Self, HillshadeRangeError>
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
            *(param.slot)(&mut self) = param.checked(value)?;
        }
        Ok(self)
    }

    /// A short, stable fingerprint of everything that affects the baked bytes.
    ///
    /// Used as part of the tile's etag; must change whenever any parameter
    /// does, or a client keeps a stale tile after a re-tune.
    #[must_use]
    #[expect(
        clippy::unneeded_field_pattern,
        reason = "the ignored fields are folded in through NUMERIC_PARAMS; naming \
                  every field anyway means adding one that affects the baked bytes \
                  without folding it in here is a compile error, which `..` would hide"
    )]
    pub fn fingerprint(&self) -> String {
        let Self {
            light:
                LightAngles {
                    azimuth_deg: _,
                    altitude_deg: _,
                },
            bake:
                BakeParams {
                    padding,
                    vertical_exaggeration: _,
                    contrast: _,
                    elevation_scale: _,
                    toon_bands: _,
                    ambient: _,
                },
            format,
            // Permission itself doesn't change the bytes; the values it lets through do.
            allow_request_overrides: _,
        } = self;

        let mut probe = *self;
        let numeric = NUMERIC_PARAMS
            .iter()
            .map(|param| (param.slot)(&mut probe).to_string())
            .collect::<Vec<_>>()
            .join(":");
        format!("{numeric}:{padding}:{format}")
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use indoc::indoc;

    use super::*;

    fn parse(yaml: &str) -> HillshadeProcessConfig {
        serde_saphyr::from_str(yaml).expect("parses")
    }

    fn opted_in() -> ResolvedHillshade {
        ResolvedHillshade {
            allow_request_overrides: true,
            ..Default::default()
        }
    }

    fn distinct_from_default(param: &NumericParam) -> f64 {
        let mut probe = ResolvedHillshade::default();
        let current = *(param.slot)(&mut probe);
        if (current - param.range.start()).abs() > 1e-9 {
            *param.range.start()
        } else {
            *param.range.end()
        }
    }

    fn line(report: &mut String, args: std::fmt::Arguments) {
        writeln!(report, "{args}").expect("writing to a String cannot fail");
    }

    #[test]
    fn auto_means_every_default() {
        let resolved = parse("auto")
            .resolve_hillshade()
            .expect("defaults are in range")
            .expect("auto is enabled");
        insta::assert_debug_snapshot!(resolved, @"
        ResolvedHillshade {
            light: LightAngles {
                azimuth_deg: 315.0,
                altitude_deg: 45.0,
            },
            bake: BakeParams {
                padding: 0,
                vertical_exaggeration: 3.0,
                contrast: 1.9,
                elevation_scale: 2.5,
                toon_bands: 3.0,
                ambient: 0.25,
            },
            format: Png,
            allow_request_overrides: false,
        }
        ");
        insta::assert_snapshot!(resolved.fingerprint(), @"315:45:3:1.9:2.5:3:0.25:0:png");
    }

    #[test]
    fn disabled_switches_hillshading_off() {
        let resolved = parse("disabled")
            .resolve_hillshade()
            .expect("disabling is valid");
        assert_eq!(resolved, None);
    }

    #[test]
    fn false_switches_hillshading_off() {
        let resolved = parse("false")
            .resolve_hillshade()
            .expect("disabling is valid");
        assert_eq!(resolved, None);
    }

    #[test]
    fn an_empty_map_is_enabled_with_defaults() {
        let resolved = parse("{}").resolve_hillshade().unwrap().unwrap();
        assert_eq!(resolved, ResolvedHillshade::default());
    }

    #[test]
    fn named_settings_override_only_themselves() {
        let resolved = parse(indoc! {"
            azimuth: 90
            toon_bands: 0
            padding: 8
            format: webp
        "})
        .resolve_hillshade()
        .unwrap()
        .unwrap();

        insta::assert_debug_snapshot!(resolved, @"
        ResolvedHillshade {
            light: LightAngles {
                azimuth_deg: 90.0,
                altitude_deg: 45.0,
            },
            bake: BakeParams {
                padding: 8,
                vertical_exaggeration: 3.0,
                contrast: 1.9,
                elevation_scale: 2.5,
                toon_bands: 0.0,
                ambient: 0.25,
            },
            format: Webp,
            allow_request_overrides: false,
        }
        ");
    }

    #[test]
    fn out_of_range_settings_are_rejected_by_name() {
        let mut report = String::new();
        for param in &NUMERIC_PARAMS {
            for value in [param.range.start() - 1.0, param.range.end() + 1.0] {
                let yaml = format!("{}: {value}", param.name);
                let err = parse(&yaml)
                    .resolve_hillshade()
                    .expect_err("out of range must be rejected");
                writeln!(report, "{yaml} -> {err}").expect("writes");
            }
        }
        let yaml = format!("padding: {}", MAX_PADDING + 1);
        let err = parse(&yaml)
            .resolve_hillshade()
            .expect_err("out of range must be rejected");
        writeln!(report, "{yaml} -> {err}").expect("writes");

        insta::assert_snapshot!(report, @"
        azimuth: -1 -> Hillshade parameter azimuth must be between `0` and `360`, but was `-1`
        azimuth: 361 -> Hillshade parameter azimuth must be between `0` and `360`, but was `361`
        altitude: -1 -> Hillshade parameter altitude must be between `0` and `90`, but was `-1`
        altitude: 91 -> Hillshade parameter altitude must be between `0` and `90`, but was `91`
        vertical_exaggeration: -1 -> Hillshade parameter vertical_exaggeration must be between `0` and `10`, but was `-1`
        vertical_exaggeration: 11 -> Hillshade parameter vertical_exaggeration must be between `0` and `10`, but was `11`
        contrast: -1 -> Hillshade parameter contrast must be between `0` and `10`, but was `-1`
        contrast: 11 -> Hillshade parameter contrast must be between `0` and `10`, but was `11`
        elevation_scale: -1 -> Hillshade parameter elevation_scale must be between `0` and `10`, but was `-1`
        elevation_scale: 11 -> Hillshade parameter elevation_scale must be between `0` and `10`, but was `11`
        toon_bands: -1 -> Hillshade parameter toon_bands must be between `0` and `32`, but was `-1`
        toon_bands: 33 -> Hillshade parameter toon_bands must be between `0` and `32`, but was `33`
        ambient: -1 -> Hillshade parameter ambient must be between `0` and `1`, but was `-1`
        ambient: 2 -> Hillshade parameter ambient must be between `0` and `1`, but was `2`
        padding: 33 -> Hillshade parameter padding must be between `0` and `32`, but was `33`
        ");
    }

    #[test]
    fn non_finite_settings_are_rejected() {
        let mut report = String::new();
        for yaml in ["ambient: .nan", "contrast: .inf", "contrast: -.inf"] {
            let err = parse(yaml)
                .resolve_hillshade()
                .expect_err("non-finite must be rejected");
            writeln!(report, "{yaml} -> {err}").expect("writes");
        }
        insta::assert_snapshot!(report, @"
        ambient: .nan -> Hillshade parameter ambient must be between `0` and `1`, but was `NaN`
        contrast: .inf -> Hillshade parameter contrast must be between `0` and `10`, but was `inf`
        contrast: -.inf -> Hillshade parameter contrast must be between `0` and `10`, but was `-inf`
        ");
    }

    #[test]
    fn a_lossy_format_cannot_be_expressed() {
        for yaml in ["format: jpeg", "format: avif"] {
            serde_saphyr::from_str::<HillshadeProcessConfig>(yaml).expect_err("must not parse");
        }
    }

    #[test]
    fn unrecognized_keys_are_collected_for_the_typo_warning() {
        let cfg = parse(indoc! {"
            azimuth: 90
            azimuht_foo: 90
        "});
        let HillshadeProcessConfig::Explicit(settings) = cfg else {
            unreachable!("a map parses as explicit settings");
        };
        let keys = settings
            .get_unrecognized_keys()
            .into_iter()
            .collect::<Vec<_>>();
        assert_eq!(keys.as_slice(), ["azimuht_foo"]);
    }

    #[test]
    fn request_overrides_apply_only_when_opted_in() {
        let mut report = String::new();
        for allow_request_overrides in [false, true] {
            let base = ResolvedHillshade {
                allow_request_overrides,
                ..Default::default()
            };
            let after = base
                .with_query_overrides([("azimuth", "180"), ("toon_bands", "0")])
                .expect("in-range overrides are accepted");
            line(
                &mut report,
                format_args!("allow={allow_request_overrides} -> {}", after.fingerprint()),
            );
        }
        insta::assert_snapshot!(report, @"
        allow=false -> 315:45:3:1.9:2.5:3:0.25:0:png
        allow=true -> 180:45:3:1.9:2.5:0:0.25:0:png
        ");
    }

    #[test]
    fn out_of_range_request_overrides_are_rejected() {
        let err = opted_in()
            .with_query_overrides([("altitude", "120")])
            .expect_err("must be rejected");
        insta::assert_snapshot!(err.to_string(), @"Hillshade parameter altitude must be between `0` and `90`, but was `120`");
    }

    #[test]
    fn query_parameters_that_are_not_ours_are_left_alone() {
        let open = opted_in();
        let after = open
            .with_query_overrides([("filter", "roads"), ("v", "3")])
            .unwrap();
        assert_eq!(after, open);
    }

    #[test]
    fn a_parameter_of_ours_with_an_unparseable_value_is_rejected() {
        let mut report = String::new();
        for raw in ["abc", "", "45deg"] {
            let err = opted_in()
                .with_query_overrides([("azimuth", raw)])
                .expect_err("must be rejected");
            writeln!(report, "azimuth={raw:?} -> {err}").expect("writes");
        }
        insta::assert_snapshot!(report, @r#"
        azimuth="abc" -> Hillshade parameter azimuth must be between `0` and `360`, but was `abc`
        azimuth="" -> Hillshade parameter azimuth must be between `0` and `360`, but was ``
        azimuth="45deg" -> Hillshade parameter azimuth must be between `0` and `360`, but was `45deg`
        "#);
    }

    #[test]
    fn every_parameter_can_be_overridden_by_a_request() {
        let base = opted_in();
        let mut report = String::new();
        for param in &NUMERIC_PARAMS {
            let target = distinct_from_default(param);
            let after = base
                .with_query_overrides([(param.name, target.to_string().as_str())])
                .expect("an in-range value is accepted");
            line(
                &mut report,
                format_args!("{}={target} -> {}", param.name, after.fingerprint()),
            );
        }
        insta::assert_snapshot!(report, @"
        azimuth=0 -> 0:45:3:1.9:2.5:3:0.25:0:png
        altitude=0 -> 315:0:3:1.9:2.5:3:0.25:0:png
        vertical_exaggeration=0 -> 315:45:0:1.9:2.5:3:0.25:0:png
        contrast=0 -> 315:45:3:0:2.5:3:0.25:0:png
        elevation_scale=0 -> 315:45:3:1.9:0:3:0.25:0:png
        toon_bands=0 -> 315:45:3:1.9:2.5:0:0.25:0:png
        ambient=0 -> 315:45:3:1.9:2.5:3:0:0:png
        ");
    }

    #[test]
    fn every_parameter_is_folded_into_the_fingerprint() {
        let base = ResolvedHillshade::default();
        let mut variants = NUMERIC_PARAMS
            .iter()
            .map(|param| {
                let mut variant = base;
                *(param.slot)(&mut variant) = distinct_from_default(param);
                (param.name, variant)
            })
            .collect::<Vec<_>>();
        variants.push((
            "padding",
            ResolvedHillshade {
                bake: BakeParams {
                    padding: 8,
                    ..base.bake
                },
                ..base
            },
        ));
        variants.push((
            "format",
            ResolvedHillshade {
                format: Format::Webp,
                ..base
            },
        ));
        variants.push(("allow_request_overrides", opted_in()));

        let mut report = String::new();
        for (name, variant) in variants {
            let verdict = if variant.fingerprint() == base.fingerprint() {
                "unchanged"
            } else {
                "changed"
            };
            writeln!(report, "{name} -> {verdict}").expect("writes");
        }
        insta::assert_snapshot!(report, @"
        azimuth -> changed
        altitude -> changed
        vertical_exaggeration -> changed
        contrast -> changed
        elevation_scale -> changed
        toon_bands -> changed
        ambient -> changed
        padding -> changed
        format -> changed
        allow_request_overrides -> unchanged
        ");
    }
}

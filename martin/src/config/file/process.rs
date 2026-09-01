#[cfg(all(feature = "contour", feature = "_tiles"))]
use martin_core::tiles::contour::ElevationUnits;
use martin_tile_utils::TileInfo;
#[cfg(all(feature = "contour", feature = "_tiles"))]
use martin_tile_utils::{Encoding, Format};
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use mlt_core::encoder::EncoderConfig;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use serde::{Deserialize, Serialize};
use tilejson::TileJSON;
#[cfg(all(feature = "contour", feature = "_tiles"))]
use tilejson::VectorLayer;

#[cfg(all(feature = "contour", feature = "_tiles"))]
use crate::config::file::contour::{ContourProcessConfig, ContourRangeError, ResolvedContour};
#[cfg(all(feature = "hillshade", feature = "_tiles"))]
use crate::config::file::hillshade::{
    HillshadeProcessConfig, HillshadeRangeError, ResolvedHillshade,
};
use crate::config::file::{CacheControlHeader, ConfigFileError};
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::{CollectUnrecognizedKeys, UnrecognizedKeys, UnrecognizedValues};
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::primitives::AutoOption;

/// One level of serving settings as written in the config, global, source-type or per-source.
///
/// Every field is `None` until that level sets it.
/// Levels fold together with [`layered`](Self::layered) and become a [`ResolvedProcess`] through [`resolve`](Self::resolve).
/// Nothing past `Config::finalize` reads this type.
///
/// Not serialized directly - config files use `convert_to_mlt` / `convert_to_mvt` /
/// `cache_control`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProcessConfig {
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    pub convert_to_mlt: Option<MltProcessConfig>,
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    pub convert_to_mvt: Option<MvtProcessConfig>,
    /// `Cache-Control` response header for this source,
    /// overriding the server-level `cache_control` default.
    pub cache_control: Option<CacheControlHeader>,
    #[cfg(all(feature = "hillshade", feature = "_tiles"))]
    pub convert_to_hillshade: Option<HillshadeProcessConfig>,
    #[cfg(all(feature = "contour", feature = "_tiles"))]
    pub convert_to_contour: Option<ContourProcessConfig>,
}

impl ProcessConfig {
    /// Folds the three levels into one, per-source over source-type over global.
    ///
    /// The `convert_to_mlt`/`convert_to_mvt` pair overrides as a unit, `cache_control` resolves on
    /// its own, and `convert_to_hillshade`/`convert_to_contour` are per-source only.
    #[must_use]
    pub fn layered(global: &Self, source_type: &Self, per_source: &Self) -> Self {
        let cache_control = per_source
            .cache_control
            .clone()
            .or_else(|| source_type.cache_control.clone())
            .or_else(|| global.cache_control.clone());
        #[cfg(all(feature = "mlt", feature = "_tiles"))]
        let conversions = [per_source, source_type, global]
            .into_iter()
            .find(|pc| pc.convert_to_mlt.is_some() || pc.convert_to_mvt.is_some())
            .unwrap_or(global);
        Self {
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            convert_to_mlt: conversions.convert_to_mlt.clone(),
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            convert_to_mvt: conversions.convert_to_mvt.clone(),
            cache_control,
            #[cfg(all(feature = "hillshade", feature = "_tiles"))]
            convert_to_hillshade: per_source.convert_to_hillshade.clone(),
            #[cfg(all(feature = "contour", feature = "_tiles"))]
            convert_to_contour: per_source.convert_to_contour.clone(),
        }
    }

    /// Applies the defaults and range-checks the settings, once.
    ///
    /// # Errors
    ///
    /// A hillshade or contour parameter outside its range.
    pub fn resolve(&self) -> Result<ResolvedProcess, ProcessResolveError> {
        Ok(ResolvedProcess {
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            mlt: match &self.convert_to_mlt {
                None | Some(MltProcessConfig::Auto) => {
                    MltConversion::Encode(EncoderConfig::default())
                }
                Some(MltProcessConfig::Explicit(cfg)) => {
                    MltConversion::Encode(EncoderConfig::from(cfg.clone()))
                }
                Some(MltProcessConfig::Disabled) => MltConversion::Disabled,
            },
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            mvt: match &self.convert_to_mvt {
                Some(MvtProcessConfig::Disabled) => MvtConversion::Disabled,
                None | Some(MvtProcessConfig::Auto | MvtProcessConfig::Explicit(_)) => {
                    MvtConversion::Encode
                }
            },
            cache_control: self.cache_control.clone(),
            #[cfg(all(feature = "hillshade", feature = "_tiles"))]
            hillshade: match &self.convert_to_hillshade {
                None => None,
                Some(config) => config.resolve_hillshade()?,
            },
            #[cfg(all(feature = "contour", feature = "_tiles"))]
            contour: match &self.convert_to_contour {
                None => None,
                Some(config) => config.resolve_contour()?,
            },
        })
    }
}

/// A setting that did not survive [`ProcessConfig::resolve`].
#[derive(Debug, thiserror::Error)]
pub enum ProcessResolveError {
    #[cfg(all(feature = "hillshade", feature = "_tiles"))]
    #[error(transparent)]
    Hillshade(#[from] HillshadeRangeError),
    #[cfg(all(feature = "contour", feature = "_tiles"))]
    #[error(transparent)]
    Contour(#[from] ContourRangeError),
}

impl ProcessResolveError {
    /// The startup error naming the source at fault.
    #[must_use]
    #[cfg_attr(
        not(any(
            all(feature = "hillshade", feature = "_tiles"),
            all(feature = "contour", feature = "_tiles")
        )),
        expect(
            unused_variables,
            reason = "nothing can fail to resolve without those features"
        )
    )]
    pub fn for_source(self, source_id: String) -> ConfigFileError {
        match self {
            #[cfg(all(feature = "hillshade", feature = "_tiles"))]
            Self::Hillshade(source) => ConfigFileError::InvalidHillshade {
                source_id,
                source: Box::new(source),
            },
            #[cfg(all(feature = "contour", feature = "_tiles"))]
            Self::Contour(source) => ConfigFileError::InvalidContour {
                source_id,
                source: Box::new(source),
            },
        }
    }
}

/// Whether, and how, MVT tiles are re-encoded as MLT for clients that accept it.
#[cfg(all(feature = "mlt", feature = "_tiles"))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MltConversion {
    /// Convert with these encoder settings.
    Encode(EncoderConfig),
    /// Serve the MVT bytes as they are.
    Disabled,
}

/// Whether MLT tiles are decoded to MVT for clients that accept only that.
#[cfg(all(feature = "mlt", feature = "_tiles"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MvtConversion {
    /// Convert.
    #[default]
    Encode,
    /// Serve the MLT bytes as they are.
    Disabled,
}

/// What a source is served with, every config level folded together and range-checked.
///
/// The catalog and the tile pipeline only ever see this type.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedProcess {
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    pub mlt: MltConversion,
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    pub mvt: MvtConversion,
    /// `Cache-Control` response header for this source.
    /// `None` leaves the server-level default to the middleware.
    pub cache_control: Option<CacheControlHeader>,
    /// Kernel parameters when the source is hillshaded.
    #[cfg(all(feature = "hillshade", feature = "_tiles"))]
    pub hillshade: Option<ResolvedHillshade>,
    /// Tracer parameters when the source is contoured.
    #[cfg(all(feature = "contour", feature = "_tiles"))]
    pub contour: Option<ResolvedContour>,
}

impl Default for ResolvedProcess {
    /// What a source gets when no level configures anything.
    fn default() -> Self {
        Self {
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            mlt: MltConversion::Encode(EncoderConfig::default()),
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            mvt: MvtConversion::Encode,
            cache_control: None,
            #[cfg(all(feature = "hillshade", feature = "_tiles"))]
            hillshade: None,
            #[cfg(all(feature = "contour", feature = "_tiles"))]
            contour: None,
        }
    }
}

impl ResolvedProcess {
    /// The [`TileInfo`] this source answers with once post-processing has run.
    ///
    /// Contouring replaces an elevation raster with a vector tile, so the format
    /// a client is told about - and the one `Accept` is negotiated against - has
    /// to be the traced one, not the source's own.
    #[must_use]
    pub fn advertised_tile_info(&self, source: TileInfo) -> TileInfo {
        #[cfg(all(feature = "contour", feature = "_tiles"))]
        if self.contour.is_some() {
            return TileInfo::new(Format::Mvt, Encoding::Uncompressed);
        }
        source
    }

    /// Whether a post-cache processor shapes this source's tiles.
    #[must_use]
    #[cfg_attr(
        not(all(any(feature = "hillshade", feature = "contour"), feature = "_tiles")),
        expect(clippy::unused_self)
    )]
    pub fn is_post_processed(&self) -> bool {
        #[cfg(all(feature = "hillshade", feature = "_tiles"))]
        if self.hillshade.is_some() {
            return true;
        }
        #[cfg(all(feature = "contour", feature = "_tiles"))]
        if self.contour.is_some() {
            return true;
        }
        false
    }

    /// The [`TileJSON`] this source answers with once post-processing has run.
    #[must_use]
    pub fn advertised_tilejson(&self, tilejson: TileJSON) -> TileJSON {
        #[cfg(all(feature = "contour", feature = "_tiles"))]
        if let Some(settings) = &self.contour {
            let mut fields = std::collections::BTreeMap::new();
            fields.insert("ele".to_owned(), "Number".to_owned());
            fields.insert("major".to_owned(), "Boolean".to_owned());
            let layer = VectorLayer {
                id: settings.opts.layer_name.clone(),
                fields,
                description: Some(match settings.opts.elevation_units {
                    ElevationUnits::Meters => "Contour lines, elevation in meters".to_owned(),
                    ElevationUnits::Feet => "Contour lines, elevation in feet".to_owned(),
                }),
                maxzoom: tilejson.maxzoom,
                minzoom: tilejson.minzoom,
                other: std::collections::BTreeMap::new(),
            };
            return TileJSON {
                vector_layers: Some(vec![layer]),
                ..tilejson
            };
        }
        tilejson
    }
}

/// Configuration for MVT-to-MLT format conversion.
///
/// Three-state value parsed from YAML:
/// - `"auto"` / `"default"` / `true` - use `mlt-core`'s default `EncoderConfig`
/// - `"disabled"` / `"off"` / `"no"` / `false` - explicitly skip conversion
/// - An object with explicit fields - override specific encoder settings
#[cfg(all(feature = "mlt", feature = "_tiles"))]
pub type MltProcessConfig = AutoOption<MltEncoderConfig>;

/// Configuration for MLT-to-MVT format conversion.
#[cfg(all(feature = "mlt", feature = "_tiles"))]
pub type MvtProcessConfig = AutoOption<MvtEncoderConfig>;

/// Explicit encoder configuration for MVT conversion.
///
/// The MVT encoder currently has no tunable knobs, so any keys provided are
/// captured here verbatim and surfaced through the established unrecognized-key
/// warning path so users get a typo hint instead of silent acceptance.
#[cfg(all(feature = "mlt", feature = "_tiles"))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct MvtEncoderConfig(pub serde_json::Map<String, serde_json::Value>);

#[cfg(all(feature = "mlt", feature = "_tiles"))]
impl CollectUnrecognizedKeys for MvtEncoderConfig {
    fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys) {
        for key in self.0.keys() {
            out.insert(format!("{path}{key}"));
        }
    }
}

/// Explicit encoder configuration for MLT conversion.
/// All fields are optional; unset fields use `mlt-core`'s defaults.
#[cfg(all(feature = "mlt", feature = "_tiles"))]
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct MltEncoderConfig {
    /// Generate tessellation data for polygons and multi-polygons.
    pub tessellate: Option<bool>,
    /// Try sorting features by Z-order (Morton) curve index of their first vertex.
    pub try_spatial_morton_sort: Option<bool>,
    /// Try sorting features by Hilbert curve index of their first vertex.
    pub try_spatial_hilbert_sort: Option<bool>,
    /// Try sorting features by their feature ID in ascending order.
    pub try_id_sort: Option<bool>,
    /// Allow FSST string compression.
    pub allow_fsst: Option<bool>,
    /// Allow `FastPFOR` integer compression.
    #[serde(alias = "allow_fpf")]
    pub allow_fastpfor: Option<bool>,
    /// Allow string grouping into shared dictionaries.
    pub allow_shared_dict: Option<bool>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

/// Applies `MltEncoderConfig` overrides on top of `EncoderConfig` defaults:
/// a set field overrides the default, an unset (`None`) field keeps it.
#[cfg(all(feature = "mlt", feature = "_tiles"))]
impl From<MltEncoderConfig> for EncoderConfig {
    fn from(src: MltEncoderConfig) -> Self {
        // Destructure so new fields cause a compile error.
        let MltEncoderConfig {
            tessellate,
            try_spatial_morton_sort,
            try_spatial_hilbert_sort,
            try_id_sort,
            allow_fsst,
            allow_fastpfor,
            allow_shared_dict,
            // Unrecognized keys are reported via the warning path during finalize();
            // they intentionally don't influence the resulting EncoderConfig.
            unrecognized: _unrecognized,
        } = src;

        let mut cfg = Self::default();
        if let Some(v) = tessellate {
            cfg = cfg.with_tessellation(v);
        }
        if let Some(v) = try_spatial_morton_sort {
            cfg = cfg.with_spatial_morton_sort(v);
        }
        if let Some(v) = try_spatial_hilbert_sort {
            cfg = cfg.with_spatial_hilbert_sort(v);
        }
        if let Some(v) = try_id_sort {
            cfg = cfg.with_id_sort(v);
        }
        if let Some(v) = allow_fsst {
            cfg = cfg.with_fsst(v);
        }
        if let Some(v) = allow_fastpfor {
            cfg = cfg.with_fastpfor(v);
        }
        if let Some(v) = allow_shared_dict {
            cfg = cfg.with_shared_dict(v);
        }
        cfg
    }
}

#[cfg(test)]
mod tests {
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    use indoc::indoc;

    use super::*;

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn parse_mlt_auto_string() {
        let cfg: MltProcessConfig = serde_saphyr::from_str("auto").unwrap();
        assert_eq!(cfg, MltProcessConfig::Auto);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn parse_mlt_explicit_empty() {
        let cfg: MltProcessConfig = serde_saphyr::from_str("{}").unwrap();
        assert_eq!(cfg, MltProcessConfig::Explicit(MltEncoderConfig::default()));
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn parse_mlt_explicit_with_overrides() {
        let cfg: MltProcessConfig = serde_saphyr::from_str(indoc! {"
            tessellate: true
            allow_fsst: false
        "})
        .unwrap();
        assert_eq!(
            cfg,
            MltProcessConfig::Explicit(MltEncoderConfig {
                tessellate: Some(true),
                allow_fsst: Some(false),
                ..Default::default()
            })
        );
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn serde_round_trip_auto() {
        let cfg = MltProcessConfig::Auto;
        let yaml = serde_saphyr::to_string(&cfg).unwrap();
        insta::assert_snapshot!(yaml, @"auto");
        let parsed: MltProcessConfig = serde_saphyr::from_str(&yaml).unwrap();
        assert_eq!(cfg, parsed);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn serde_round_trip_disabled() {
        let cfg = MltProcessConfig::Disabled;
        let yaml = serde_saphyr::to_string(&cfg).unwrap();
        insta::assert_snapshot!(yaml, @"disabled");
        let parsed: MltProcessConfig = serde_saphyr::from_str(&yaml).unwrap();
        assert_eq!(cfg, parsed);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn serde_round_trip_explicit() {
        let cfg = MltProcessConfig::Explicit(MltEncoderConfig {
            tessellate: Some(true),
            ..Default::default()
        });
        let yaml = serde_saphyr::to_string(&cfg).unwrap();
        let parsed: MltProcessConfig = serde_saphyr::from_str(&yaml).unwrap();
        assert_eq!(cfg, parsed);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn parse_mlt_invalid_string() {
        let result = serde_saphyr::from_str::<MltProcessConfig>("invalid");
        result.unwrap_err();
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn parse_mlt_invalid_type() {
        let result = serde_saphyr::from_str::<MltProcessConfig>("123");
        result.unwrap_err();
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn render_failure_mlt_unknown_string() {
        use crate::config::test_helpers::render_failure;
        insta::assert_snapshot!(render_failure(indoc! {"
                convert_to_mlt: atuo
            "}), @r#"
        martin::config::yaml (https://maplibre.org/martin/config-file/)

          × invalid value: string "atuo", expected a string ("auto", "enabled",
          │ "disabled"), a boolean, or a map of settings
           ╭─[config.yaml:1:1]
         1 │ convert_to_mlt: atuo
           · ───────┬──────
           ·        ╰── invalid value: string "atuo", expected a string ("auto", "enabled", "disabled"), a boolean, or a map of settings
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        "#);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn render_failure_mlt_integer() {
        use crate::config::test_helpers::render_failure;
        insta::assert_snapshot!(render_failure(indoc! {"
                convert_to_mlt: 42
            "}), @r#"
        martin::config::yaml (https://maplibre.org/martin/config-file/)

          × invalid type: integer `42`, expected a string ("auto", "enabled",
          │ "disabled"), a boolean, or a map of settings
           ╭─[config.yaml:1:1]
         1 │ convert_to_mlt: 42
           · ───────┬──────
           ·        ╰── invalid type: integer `42`, expected a string ("auto", "enabled", "disabled"), a boolean, or a map of settings
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        "#);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn render_failure_mlt_nested_field_bad_type() {
        use crate::config::test_helpers::render_failure;
        insta::assert_snapshot!(render_failure(indoc! {"
                convert_to_mlt:
                  tessellate: yes-please
            "}), @"
        martin::config::yaml (https://maplibre.org/martin/config-file/)

          × invalid boolean
           ╭─[config.yaml:2:15]
         1 │ convert_to_mlt:
         2 │   tessellate: yes-please
           ·               ─────┬────
           ·                    ╰── invalid boolean
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        ");
    }

    #[cfg(all(feature = "mlt", feature = "hillshade", feature = "_tiles"))]
    #[test]
    fn resolve_per_source_disabled_overrides_global_auto() {
        let global = ProcessConfig {
            convert_to_mlt: Some(MltProcessConfig::Auto),
            convert_to_mvt: None,
            cache_control: None,
            convert_to_hillshade: None,
            #[cfg(all(feature = "contour", feature = "_tiles"))]
            convert_to_contour: None,
        };
        let per_source = ProcessConfig {
            convert_to_mlt: Some(MltProcessConfig::Disabled),
            convert_to_mvt: None,
            cache_control: None,
            convert_to_hillshade: None,
            #[cfg(all(feature = "contour", feature = "_tiles"))]
            convert_to_contour: None,
        };
        let resolved = ProcessConfig::layered(&global, &ProcessConfig::default(), &per_source);
        assert_eq!(resolved.convert_to_mlt, Some(MltProcessConfig::Disabled));
    }

    #[cfg(all(feature = "mlt", feature = "hillshade", feature = "_tiles"))]
    #[test]
    fn resolve_per_source_overrides_all() {
        let global = ProcessConfig {
            convert_to_mlt: Some(MltProcessConfig::Auto),
            convert_to_mvt: None,
            cache_control: None,
            convert_to_hillshade: None,
            #[cfg(all(feature = "contour", feature = "_tiles"))]
            convert_to_contour: None,
        };
        let source_type = ProcessConfig {
            convert_to_mlt: None,
            convert_to_mvt: Some(MvtProcessConfig::Auto),
            cache_control: None,
            convert_to_hillshade: None,
            #[cfg(all(feature = "contour", feature = "_tiles"))]
            convert_to_contour: None,
        };
        let per_source = ProcessConfig {
            convert_to_mlt: Some(MltProcessConfig::Explicit(MltEncoderConfig {
                tessellate: Some(true),
                ..Default::default()
            })),
            convert_to_mvt: None,
            cache_control: None,
            convert_to_hillshade: None,
            #[cfg(all(feature = "contour", feature = "_tiles"))]
            convert_to_contour: None,
        };

        let resolved = ProcessConfig::layered(&global, &source_type, &per_source);
        assert_eq!(resolved, per_source);
    }

    #[cfg(all(feature = "mlt", feature = "hillshade", feature = "_tiles"))]
    #[test]
    fn resolve_source_type_overrides_global() {
        let global = ProcessConfig {
            convert_to_mlt: Some(MltProcessConfig::Auto),
            convert_to_mvt: None,
            cache_control: None,
            convert_to_hillshade: None,
            #[cfg(all(feature = "contour", feature = "_tiles"))]
            convert_to_contour: None,
        };
        let source_type = ProcessConfig {
            convert_to_mlt: None,
            convert_to_mvt: Some(MvtProcessConfig::Auto),
            cache_control: None,
            convert_to_hillshade: None,
            #[cfg(all(feature = "contour", feature = "_tiles"))]
            convert_to_contour: None,
        };

        let resolved = ProcessConfig::layered(&global, &source_type, &ProcessConfig::default());
        assert_eq!(resolved, source_type);
    }

    #[cfg(all(feature = "mlt", feature = "hillshade", feature = "_tiles"))]
    #[test]
    fn resolve_global_used_as_fallback() {
        let global = ProcessConfig {
            convert_to_mlt: Some(MltProcessConfig::Auto),
            convert_to_mvt: None,
            cache_control: None,
            convert_to_hillshade: None,
            #[cfg(all(feature = "contour", feature = "_tiles"))]
            convert_to_contour: None,
        };

        let resolved = ProcessConfig::layered(
            &global,
            &ProcessConfig::default(),
            &ProcessConfig::default(),
        );
        assert_eq!(resolved, global);
    }

    fn cache_control(value: &str) -> CacheControlHeader {
        serde_saphyr::from_str(value).expect("valid Cache-Control header")
    }

    fn only_cache_control(value: &str) -> ProcessConfig {
        ProcessConfig {
            cache_control: Some(cache_control(value)),
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            convert_to_mlt: None,
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            convert_to_mvt: None,
            #[cfg(feature = "hillshade")]
            convert_to_hillshade: None,
            #[cfg(feature = "contour")]
            convert_to_contour: None,
        }
    }

    #[test]
    fn resolve_per_source_cache_control_wins() {
        let source_type = only_cache_control("public, max-age=60");
        let per_source = only_cache_control("no-store");
        let resolved = ProcessConfig::layered(&ProcessConfig::default(), &source_type, &per_source);
        assert_eq!(resolved.cache_control, Some(cache_control("no-store")));
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn resolve_cache_control_independently_of_conversions() {
        let source_type = ProcessConfig {
            convert_to_mlt: Some(MltProcessConfig::Disabled),
            cache_control: Some(cache_control("public, max-age=60")),
            ..Default::default()
        };
        let per_source = ProcessConfig {
            convert_to_mlt: Some(MltProcessConfig::Auto),
            ..Default::default()
        };
        let resolved = ProcessConfig::layered(&ProcessConfig::default(), &source_type, &per_source);
        assert_eq!(resolved.convert_to_mlt, Some(MltProcessConfig::Auto));
        assert_eq!(
            resolved.cache_control,
            Some(cache_control("public, max-age=60"))
        );

        let resolved = ProcessConfig::layered(
            &ProcessConfig::default(),
            &source_type,
            &only_cache_control("no-store"),
        );
        assert_eq!(resolved.convert_to_mlt, Some(MltProcessConfig::Disabled));
        assert_eq!(resolved.cache_control, Some(cache_control("no-store")));
    }

    #[test]
    fn resolve_default_when_all_none() {
        let resolved = ProcessConfig::layered(
            &ProcessConfig::default(),
            &ProcessConfig::default(),
            &ProcessConfig::default(),
        );
        assert_eq!(resolved, ProcessConfig::default());
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn mlt_encoder_captures_unrecognized_keys() {
        let cfg: MltProcessConfig = serde_saphyr::from_str(indoc! {"
            tessellate: true
            unknown_knob: 42
            another_typo: hi
        "})
        .unwrap();
        let MltProcessConfig::Explicit(inner) = cfg else {
            panic!("expected explicit MltEncoderConfig");
        };
        assert_eq!(inner.tessellate, Some(true));
        let collected = inner.get_unrecognized_keys();
        let mut keys: Vec<&str> = collected.iter().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["another_typo", "unknown_knob"]);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn mvt_encoder_captures_all_keys_as_unrecognized() {
        // MVT has no encoder knobs yet; every supplied key is unrecognized.
        let cfg: MvtProcessConfig = serde_saphyr::from_str(indoc! {"
            anything: 1
            else: yes
        "})
        .unwrap();
        let MvtProcessConfig::Explicit(inner) = cfg else {
            panic!("expected explicit MvtEncoderConfig");
        };
        let collected = inner.get_unrecognized_keys();
        let mut keys: Vec<&str> = collected.iter().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["anything", "else"]);
    }

    /// Even an empty map should produce `Explicit(MvtEncoderConfig::default())`,
    /// matching the existing behavior for the MLT side.
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn parse_mvt_explicit_empty() {
        let cfg: MvtProcessConfig = serde_saphyr::from_str("{}").unwrap();
        assert_eq!(cfg, MvtProcessConfig::Explicit(MvtEncoderConfig::default()));
    }

    /// Unknown keys inside `convert_to_mlt` should bubble up through
    /// `PmtConfig::get_unrecognized_keys` with the proper prefix so the existing
    /// warning loop in `Config::finalize` flags them.
    #[cfg(all(feature = "mlt", feature = "pmtiles"))]
    #[test]
    fn pmt_config_propagates_mlt_unrecognized_keys() {
        use crate::config::file::pmtiles::PmtConfig;

        let cfg: PmtConfig = serde_saphyr::from_str(indoc! {"
            convert_to_mlt:
              tessellate: true
              bogus_option: 1
        "})
        .unwrap();
        let keys = cfg.get_unrecognized_keys();
        assert!(
            keys.contains("convert_to_mlt.bogus_option"),
            "expected convert_to_mlt.bogus_option in {keys:?}"
        );
    }

    /// Unknown keys inside `convert_to_mvt` (MVT has no real knobs) should bubble
    /// up through `MbtConfig::get_unrecognized_keys`.
    #[cfg(all(feature = "mlt", feature = "mbtiles"))]
    #[test]
    fn mbt_config_propagates_mvt_unrecognized_keys() {
        use crate::config::file::mbtiles::MbtConfig;

        let cfg: MbtConfig = serde_saphyr::from_str(indoc! {"
            convert_to_mvt:
              not_a_real_setting: yes
        "})
        .unwrap();
        let keys = cfg.get_unrecognized_keys();
        assert!(
            keys.contains("convert_to_mvt.not_a_real_setting"),
            "expected convert_to_mvt.not_a_real_setting in {keys:?}"
        );
    }

    /// End-to-end: an unrecognized key inside `convert_to_mlt` at the top of the
    /// config file makes it into the rendered warning aggregate produced by
    /// `Config::finalize`. Needs at least one tile source so `finalize` doesn't
    /// short-circuit with `NoSources`.
    #[cfg(all(feature = "mlt", feature = "pmtiles"))]
    #[tokio::test]
    async fn finalize_collects_global_convert_to_mlt_unrecognized() {
        use crate::config::file::Config;

        let mut cfg: Config = serde_saphyr::from_str(indoc! {"
            pmtiles:
              paths: /tmp/never-read.pmtiles
            convert_to_mlt:
              tessellate: true
              definitely_a_typo: 1
        "})
        .unwrap();
        cfg.finalize().await.expect("finalize should not error");
        let keys = cfg.get_unrecognized_keys();
        assert!(
            keys.contains("convert_to_mlt.definitely_a_typo"),
            "expected convert_to_mlt.definitely_a_typo in {keys:?}"
        );
    }

    /// Pins the schema shape to the wire format. With the `AutoOption` migration the
    /// schema includes string aliases for `auto`/`default`/`true`,
    /// `disabled`/`off`/`no`/`false`, a boolean shorthand, and the explicit
    /// `MltEncoderConfig` branch - four `oneOf` entries in total.
    #[cfg(all(feature = "mlt", feature = "unstable-schemas"))]
    #[test]
    fn json_schema_matches_serde_format() {
        let schema = serde_json::to_value(schemars::schema_for!(MltProcessConfig)).unwrap();
        let one_of = schema
            .get("oneOf")
            .and_then(|v| v.as_array())
            .expect("MltProcessConfig schema should be a `oneOf`");
        assert_eq!(one_of.len(), 4, "schema: {schema}");

        // The explicit branch should still reference MltEncoderConfig.
        let mut saw_encoder_ref = false;
        for entry in one_of {
            if let Some(reference) = entry.get("$ref").and_then(|v| v.as_str())
                && reference.ends_with("/MltEncoderConfig")
            {
                saw_encoder_ref = true;
            }
        }
        assert!(
            saw_encoder_ref,
            "expected $ref to MltEncoderConfig: {schema}"
        );
    }

    #[test]
    fn resolve_of_nothing_configured_is_the_default() {
        assert_eq!(
            ProcessConfig::default().resolve().unwrap(),
            ResolvedProcess::default()
        );
    }

    #[cfg(all(feature = "hillshade", feature = "_tiles"))]
    #[test]
    fn resolve_range_checks_the_hillshade() {
        use crate::config::file::hillshade::{HillshadeProcessConfig, HillshadeSettings};

        let out_of_range = ProcessConfig {
            convert_to_hillshade: Some(HillshadeProcessConfig::Explicit(HillshadeSettings {
                azimuth: Some(400.0),
                ..Default::default()
            })),
            ..Default::default()
        };
        let err = out_of_range
            .resolve()
            .unwrap_err()
            .for_source("dem".to_owned());
        insta::assert_snapshot!(err, @"Source dem has an invalid hillshade configuration: Hillshade parameter azimuth must be between `0` and `360`, but was `400`");
    }
}

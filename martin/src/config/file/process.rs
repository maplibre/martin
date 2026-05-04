#[cfg(all(feature = "mlt", feature = "_tiles"))]
use mlt_core::encoder::EncoderConfig;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use serde::{Deserialize, Serialize};

#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::primitives::AutoOption;

/// Internal carrier for resolved per-source processing settings.
///
/// Not serialized directly — config files use `convert-to-mlt` at each level
/// instead of a nested `process` object.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProcessConfig {
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    pub convert_to_mlt: Option<MltProcessConfig>,
}

/// Configuration for MVT-to-MLT format conversion.
///
/// Three-state value parsed from YAML:
/// - `"auto"` / `"default"` / `true` — use `mlt-core`'s default `EncoderConfig`
/// - `"disabled"` / `"off"` / `"no"` / `false` — explicitly skip conversion
/// - An object with explicit fields — override specific encoder settings
#[cfg(all(feature = "mlt", feature = "_tiles"))]
pub type MltProcessConfig = AutoOption<MltEncoderConfig>;

/// Explicit encoder configuration for MLT conversion.
/// All fields are optional; unset fields use `mlt-core`'s defaults.
#[cfg(all(feature = "mlt", feature = "_tiles"))]
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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
    pub allow_fpf: Option<bool>,
    /// Allow string grouping into shared dictionaries.
    pub allow_shared_dict: Option<bool>,
}

/// Applying `MltEncoderConfig` overrides on top of `EncoderConfig` defaults.
///
/// Uses exhaustive destructuring of both structs so that adding a field
/// to either `MltEncoderConfig` or `EncoderConfig` causes a compile error
/// until this conversion is updated.
#[cfg(all(feature = "mlt", feature = "_tiles"))]
impl From<MltEncoderConfig> for EncoderConfig {
    fn from(src: MltEncoderConfig) -> Self {
        // Destructure both so new fields cause a compile error.
        let MltEncoderConfig {
            tessellate,
            try_spatial_morton_sort,
            try_spatial_hilbert_sort,
            try_id_sort,
            allow_fsst,
            allow_fpf,
            allow_shared_dict,
        } = src;

        Self {
            tessellate: tessellate.unwrap_or(Self::default().tessellate),
            try_spatial_morton_sort: try_spatial_morton_sort
                .unwrap_or(Self::default().try_spatial_morton_sort),
            try_spatial_hilbert_sort: try_spatial_hilbert_sort
                .unwrap_or(Self::default().try_spatial_hilbert_sort),
            try_id_sort: try_id_sort.unwrap_or(Self::default().try_id_sort),
            allow_fsst: allow_fsst.unwrap_or(Self::default().allow_fsst),
            allow_fpf: allow_fpf.unwrap_or(Self::default().allow_fpf),
            allow_shared_dict: allow_shared_dict.unwrap_or(Self::default().allow_shared_dict),
        }
    }
}

/// Resolve effective process config using full-override semantics:
/// per-source > source-type > global > default.
#[must_use]
pub fn resolve_process_config(
    global: Option<&ProcessConfig>,
    source_type: Option<&ProcessConfig>,
    per_source: Option<&ProcessConfig>,
) -> ProcessConfig {
    per_source
        .or(source_type)
        .or(global)
        .cloned()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    use indoc::indoc;

    use super::*;

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn parse_mlt_auto_string() {
        let cfg: MltProcessConfig = serde_yaml::from_str("auto").unwrap();
        assert_eq!(cfg, MltProcessConfig::Auto);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn parse_mlt_disabled_string() {
        for input in ["disabled", "off", "no", "false"] {
            let cfg: MltProcessConfig = serde_yaml::from_str(input).unwrap();
            assert_eq!(cfg, MltProcessConfig::Disabled, "input: {input}");
        }
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn parse_mlt_explicit_empty() {
        let cfg: MltProcessConfig = serde_yaml::from_str("{}").unwrap();
        assert_eq!(cfg, MltProcessConfig::Explicit(MltEncoderConfig::default()));
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn parse_mlt_explicit_with_overrides() {
        let cfg: MltProcessConfig = serde_yaml::from_str(indoc! {"
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
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        insta::assert_snapshot!(yaml, @"auto");
        let parsed: MltProcessConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(cfg, parsed);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn serde_round_trip_disabled() {
        let cfg = MltProcessConfig::Disabled;
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        insta::assert_snapshot!(yaml, @"disabled");
        let parsed: MltProcessConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(cfg, parsed);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn serde_round_trip_explicit() {
        let cfg = MltProcessConfig::Explicit(MltEncoderConfig {
            tessellate: Some(true),
            ..Default::default()
        });
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        let parsed: MltProcessConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(cfg, parsed);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn parse_mlt_invalid_string() {
        let result = serde_yaml::from_str::<MltProcessConfig>("invalid");
        assert!(result.is_err());
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn parse_mlt_invalid_type() {
        let result = serde_yaml::from_str::<MltProcessConfig>("123");
        assert!(result.is_err());
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn render_failure_mlt_unknown_string() {
        use crate::config::test_helpers::render_failure;
        insta::assert_snapshot!(render_failure(indoc! {"
                convert-to-mlt: atuo
            "}), @r#"
          × invalid value: string "atuo", expected a string ("auto", "enabled",
          │ "disabled"), a boolean, or a map of settings
           ╭─[config.yaml:1:1]
         1 │ convert-to-mlt: atuo
           · ───────┬──────
           ·        ╰── invalid value: string "atuo", expected a string ("auto", "enabled", "disabled"), a boolean, or a map of settings
           ╰────
        "#);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn render_failure_mlt_integer() {
        use crate::config::test_helpers::render_failure;
        insta::assert_snapshot!(render_failure(indoc! {"
                convert-to-mlt: 42
            "}), @r#"
          × invalid type: integer `42`, expected a string ("auto", "enabled",
          │ "disabled"), a boolean, or a map of settings
           ╭─[config.yaml:1:1]
         1 │ convert-to-mlt: 42
           · ───────┬──────
           ·        ╰── invalid type: integer `42`, expected a string ("auto", "enabled", "disabled"), a boolean, or a map of settings
           ╰────
        "#);
    }

    /// Inner-field errors must point at the *value*, not the outer `convert-to-mlt:` line —
    /// proves the explicit branch hands the saphyr deserializer to `MltEncoderConfig`
    /// instead of routing through a `serde_yaml::Value`.
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn render_failure_mlt_nested_field_bad_type() {
        use crate::config::test_helpers::render_failure;
        insta::assert_snapshot!(render_failure(indoc! {"
                convert-to-mlt:
                  tessellate: yes-please
            "}), @r"
          × invalid boolean
           ╭─[config.yaml:2:15]
         1 │ convert-to-mlt:
         2 │   tessellate: yes-please
           ·               ─────┬────
           ·                    ╰── invalid boolean
           ╰────
        ");
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn resolve_per_source_overrides_all() {
        let global = ProcessConfig {
            convert_to_mlt: Some(MltProcessConfig::Auto),
        };
        let source_type = ProcessConfig {
            convert_to_mlt: None,
        };
        let per_source = ProcessConfig {
            convert_to_mlt: None,
        };

        let resolved = resolve_process_config(Some(&global), Some(&source_type), Some(&per_source));
        assert_eq!(resolved, per_source);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn resolve_per_source_disabled_overrides_global_auto() {
        let global = ProcessConfig {
            convert_to_mlt: Some(MltProcessConfig::Auto),
        };
        let per_source = ProcessConfig {
            convert_to_mlt: Some(MltProcessConfig::Disabled),
        };
        let resolved = resolve_process_config(Some(&global), None, Some(&per_source));
        assert_eq!(resolved.convert_to_mlt, Some(MltProcessConfig::Disabled));
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn resolve_source_type_overrides_global() {
        let global = ProcessConfig {
            convert_to_mlt: Some(MltProcessConfig::Auto),
        };
        let source_type = ProcessConfig {
            convert_to_mlt: None,
        };

        let resolved = resolve_process_config(Some(&global), Some(&source_type), None);
        assert_eq!(resolved, source_type);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn resolve_global_used_as_fallback() {
        let global = ProcessConfig {
            convert_to_mlt: Some(MltProcessConfig::Auto),
        };

        let resolved = resolve_process_config(Some(&global), None, None);
        assert_eq!(resolved, global);
    }

    #[test]
    fn resolve_default_when_all_none() {
        let resolved = resolve_process_config(None, None, None);
        assert_eq!(resolved, ProcessConfig::default());
    }

    /// Pins the schema shape to the wire format. With the `AutoOption` migration the
    /// schema includes string aliases for `auto`/`default`/`true`,
    /// `disabled`/`off`/`no`/`false`, a boolean shorthand, and the explicit
    /// `MltEncoderConfig` branch — four `oneOf` entries in total.
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
}

#[cfg(all(feature = "mlt", feature = "_tiles"))]
use mlt_core::encoder::EncoderConfig;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use serde::{Deserialize, Serialize};

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
/// - `"auto"` — use `mlt-core`'s default `EncoderConfig`
/// - An object with explicit fields — override specific encoder settings
///
/// Deserialized from either the string `"auto"` or a config object.
#[cfg(all(feature = "mlt", feature = "_tiles"))]
#[derive(Clone, Debug, Default, PartialEq)]
pub enum MltProcessConfig {
    /// Use default encoder settings.
    #[default]
    Auto,
    /// Explicit encoder configuration overrides.
    Explicit(MltEncoderConfig),
}

// The derive would describe the Rust enum shape (`"Auto"` / `{ "Explicit": ... }`),
// but the hand-written serde impls accept `"auto"` or a bare `MltEncoderConfig`.
// Schema must follow the wire format.
#[cfg(all(feature = "mlt", feature = "unstable-schemas"))]
impl schemars::JsonSchema for MltProcessConfig {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "MltProcessConfig".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let encoder = generator.subschema_for::<MltEncoderConfig>();
        schemars::json_schema!({
            "description": "Configuration for MVT-to-MLT format conversion.\n\n\
                            - `\"auto\"` — use `mlt-core`'s default `EncoderConfig`\n\
                            - An object with explicit fields — override specific encoder settings",
            "oneOf": [
                {
                    "type": "string",
                    "const": "auto",
                    "description": "Use default encoder settings."
                },
                encoder,
            ]
        })
    }
}

#[cfg(all(feature = "mlt", feature = "_tiles"))]
impl Serialize for MltProcessConfig {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => serializer.serialize_str("auto"),
            Self::Explicit(cfg) => cfg.serialize(serializer),
        }
    }
}

// Drives the deserializer directly so saphyr's source spans survive into miette.
// Going through `serde_yaml::Value` or `Error::custom` strings would strip them.
#[cfg(all(feature = "mlt", feature = "_tiles"))]
impl<'de> Deserialize<'de> for MltProcessConfig {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use std::fmt;

        use serde::de::value::MapAccessDeserializer;
        use serde::de::{self, MapAccess, Unexpected, Visitor};

        struct MltVisitor;

        impl<'de> Visitor<'de> for MltVisitor {
            type Value = MltProcessConfig;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(r#"the string "auto" or a map of encoder settings"#)
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                if v == "auto" {
                    Ok(MltProcessConfig::Auto)
                } else {
                    Err(E::invalid_value(Unexpected::Str(v), &self))
                }
            }

            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                self.visit_str(&v)
            }

            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
                let cfg = MltEncoderConfig::deserialize(MapAccessDeserializer::new(map))?;
                Ok(MltProcessConfig::Explicit(cfg))
            }
        }

        deserializer.deserialize_any(MltVisitor)
    }
}

#[cfg(all(feature = "mlt", feature = "_tiles"))]
impl From<&MltProcessConfig> for EncoderConfig {
    fn from(src: &MltProcessConfig) -> Self {
        match src {
            MltProcessConfig::Auto => Self::default(),
            MltProcessConfig::Explicit(cfg) => Self::from(cfg.clone()),
        }
    }
}

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
        insta::assert_snapshot!(
            render_failure(indoc! {"
                convert-to-mlt: atuo
            "}),
        );
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn render_failure_mlt_integer() {
        use crate::config::test_helpers::render_failure;
        insta::assert_snapshot!(
            render_failure(indoc! {"
                convert-to-mlt: 42
            "}),
        );
    }

    /// Inner-field errors must point at the *value*, not the outer `convert-to-mlt:` line —
    /// proves the explicit branch hands the saphyr deserializer to `MltEncoderConfig`
    /// instead of routing through a `serde_yaml::Value`.
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn render_failure_mlt_nested_field_bad_type() {
        use crate::config::test_helpers::render_failure;
        insta::assert_snapshot!(
            render_failure(indoc! {"
                convert-to-mlt:
                  tessellate: yes-please
            "}),
        );
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn resolve_per_source_overrides_all() {
        let global = ProcessConfig {
            convert_to_mlt: Some(MltProcessConfig::Auto),
        };
        let source_type = ProcessConfig { convert_to_mlt: None };
        let per_source = ProcessConfig { convert_to_mlt: None };

        let resolved = resolve_process_config(Some(&global), Some(&source_type), Some(&per_source));
        assert_eq!(resolved, per_source);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn resolve_source_type_overrides_global() {
        let global = ProcessConfig {
            convert_to_mlt: Some(MltProcessConfig::Auto),
        };
        let source_type = ProcessConfig { convert_to_mlt: None };

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

    /// Pins the schema shape to the wire format (`"auto"` or a bare `MltEncoderConfig`),
    /// so a future revert to `#[derive(JsonSchema)]` — which would describe the Rust
    /// enum layout instead — fails loudly.
    #[cfg(all(feature = "mlt", feature = "unstable-schemas"))]
    #[test]
    fn json_schema_matches_serde_format() {
        let schema = serde_json::to_value(schemars::schema_for!(MltProcessConfig)).unwrap();
        let one_of = schema
            .get("oneOf")
            .and_then(|v| v.as_array())
            .expect("MltProcessConfig schema should be a `oneOf`");
        assert_eq!(one_of.len(), 2, "schema: {schema}");

        let auto = &one_of[0];
        assert_eq!(auto.get("const").and_then(|v| v.as_str()), Some("auto"));
        assert_eq!(auto.get("type").and_then(|v| v.as_str()), Some("string"));

        let explicit = &one_of[1];
        let reference = explicit
            .get("$ref")
            .and_then(|v| v.as_str())
            .expect("explicit variant should be a $ref to MltEncoderConfig");
        assert!(
            reference.ends_with("/MltEncoderConfig"),
            "expected $ref to MltEncoderConfig, got {reference}"
        );
    }
}

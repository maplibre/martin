use std::fmt;

use actix_http::Method;
use serde::de::value::MapAccessDeserializer;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_saphyr::Spanned;
use tracing::info;

use crate::config::file::{
    ConfigFileError, ConfigFileResult, ConfigurationLivecycleHooks, UnrecognizedKeys,
    UnrecognizedValues,
};
use crate::{MartinError, MartinResult};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum CorsConfig {
    Properties(CorsProperties),
    SimpleFlag(bool),
}

impl<'de> Deserialize<'de> for CorsConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CorsVisitor;

        impl<'de> Visitor<'de> for CorsVisitor {
            type Value = CorsConfig;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    "either a boolean (`cors: true` / `cors: false`) or a properties map \
                     with at least an `origin` list",
                )
            }

            fn visit_bool<E: de::Error>(self, value: bool) -> Result<CorsConfig, E> {
                Ok(CorsConfig::SimpleFlag(value))
            }

            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<CorsConfig, M::Error> {
                let props = CorsProperties::deserialize(MapAccessDeserializer::new(map))?;
                Ok(CorsConfig::Properties(props))
            }

            // Other inputs (string, number, sequence, …) fall through to serde's default,
            // which emits `de::Error::invalid_type` - saphyr attaches the source span to that
            // variant, so we get a labelled diagnostic for free.
        }

        deserializer.deserialize_any(CorsVisitor)
    }
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self::SimpleFlag(true)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct CorsProperties {
    /// Sets the `Access-Control-Allow-Origin` header \[default: *\]
    /// '*' will use the requests `ORIGIN` header
    #[serde(default = "default_spanned_empty_vec")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(example = cors_origin_example())
    )]
    pub origin: Spanned<Vec<String>>,
    /// Sets `Access-Control-Max-Age` Header. \[default: null\]
    /// null means not setting the header for preflight requests
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &3600usize))]
    pub max_age: Option<usize>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

fn default_spanned_empty_vec() -> Spanned<Vec<String>> {
    Spanned::new(
        Vec::new(),
        serde_saphyr::Location::UNKNOWN,
        serde_saphyr::Location::UNKNOWN,
    )
}

#[cfg(feature = "unstable-schemas")]
fn cors_origin_example() -> Vec<String> {
    vec!["https://example.org".to_string()]
}

impl PartialEq for CorsProperties {
    fn eq(&self, other: &Self) -> bool {
        self.origin.value == other.origin.value && self.max_age == other.max_age
    }
}

impl Eq for CorsProperties {}

impl Default for CorsProperties {
    fn default() -> Self {
        Self {
            origin: Spanned::new(
                vec!["*".to_string()],
                serde_saphyr::Location::UNKNOWN,
                serde_saphyr::Location::UNKNOWN,
            ),
            max_age: None,
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

impl ConfigurationLivecycleHooks for CorsProperties {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}

impl CorsProperties {
    pub fn validate(
        &self,
        named_source: Option<&miette::NamedSource<String>>,
    ) -> ConfigFileResult<()> {
        if self.origin.value.is_empty() {
            let span = named_source.and_then(|ns| {
                crate::config::file::error::ValidationSpan::from_location(
                    ns,
                    &self.origin.referenced,
                    "origin list is empty",
                )
            });
            Err(ConfigFileError::CorsNoOriginsConfigured(span.map(Box::new)))
        } else {
            Ok(())
        }
    }
}

impl CorsConfig {
    /// Log the current configuration
    pub fn log_current_configuration(&self) {
        match &self {
            Self::SimpleFlag(false) => info!("CORS is disabled"),
            Self::SimpleFlag(true) => info!(
                "CORS enabled with defaults: {:?}",
                CorsProperties::default()
            ),
            Self::Properties(props) => {
                info!("CORS enabled with custom properties: {props:?}");
            }
        }
    }

    /// Checks that that if cors is configured explicitly (instead of via `true`/`false`), `origin` is configured
    pub fn validate(&self, named_source: Option<&miette::NamedSource<String>>) -> MartinResult<()> {
        match self {
            Self::SimpleFlag(_) => Ok(()),
            Self::Properties(properties) => {
                properties.validate(named_source).map_err(MartinError::from)
            }
        }
    }

    #[must_use]
    /// Create [`actix_cors::Cors`] from the configuration
    pub fn make_cors_middleware(&self) -> Option<actix_cors::Cors> {
        match self {
            Self::SimpleFlag(false) => None,
            Self::SimpleFlag(true) => {
                let properties = CorsProperties::default();
                Some(Self::create_cors(&properties))
            }
            Self::Properties(properties) => Some(Self::create_cors(properties)),
        }
    }

    fn create_cors(properties: &CorsProperties) -> actix_cors::Cors {
        let mut cors = actix_cors::Cors::default();

        // allow any origin by default
        // this returns the value of the requests `ORIGIN` header in `Access-Control-Allow-Origin`
        if properties.origin.value.contains(&"*".to_string()) {
            cors = cors.allow_any_origin();
        } else {
            for origin in &properties.origin.value {
                cors = cors.allowed_origin(origin);
            }
        }

        // only allow GET method by default
        cors = cors.allowed_methods([Method::GET]);

        // sets `Access-Control-Max-Age` if configured
        cors = cors.max_age(properties.max_age);

        cors
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;
    use crate::config::test_helpers::{parse_yaml, render_failure};

    // ----- Custom `Deserialize` impl: every accepted shape and every error path -----
    //
    // Failure cases run through the full `parse_config` pipeline so the snapshot includes
    // the same graphical miette diagnostic (file path, line number, source snippet, caret,
    // help text) the user sees on the command line. Success cases use `parse_yaml` directly
    // since round-tripping through `Config` would obscure which variant was selected.

    #[test]
    fn deserialize_bool_true() {
        let cfg = parse_yaml::<CorsConfig>("true");
        assert_eq!(cfg, CorsConfig::SimpleFlag(true));
    }

    #[test]
    fn deserialize_bool_false() {
        let cfg = parse_yaml::<CorsConfig>("false");
        assert_eq!(cfg, CorsConfig::SimpleFlag(false));
    }

    #[test]
    fn deserialize_properties_map() {
        let cfg = parse_yaml::<CorsConfig>(indoc! {"
            origin:
              - https://example.org
            max_age: 3600
        "});
        let CorsConfig::Properties(props) = cfg else {
            panic!("expected Properties variant");
        };
        assert_eq!(props.origin.value, vec!["https://example.org".to_string()]);
        assert_eq!(props.max_age, Some(3600));
    }

    #[test]
    fn deserialize_rejects_integer() {
        insta::assert_snapshot!(render_failure("cors: 42\n"), @"
        martin::config::yaml (https://maplibre.org/martin/config-file/)

          × invalid type: integer `42`, expected either a boolean (`cors: true` /
          │ `cors: false`) or a properties map with at least an `origin` list
           ╭─[config.yaml:1:1]
         1 │ cors: 42
           · ──┬─
           ·   ╰── invalid type: integer `42`, expected either a boolean (`cors: true` / `cors: false`) or a properties map with at least an `origin` list
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        ");
    }

    #[test]
    fn deserialize_rejects_quoted_string() {
        insta::assert_snapshot!(render_failure("cors: \"yes please\"\n"), @r#"
        martin::config::yaml (https://maplibre.org/martin/config-file/)

          × invalid type: string "yes please", expected either a boolean (`cors:
          │ true` / `cors: false`) or a properties map with at least an `origin` list
           ╭─[config.yaml:1:1]
         1 │ cors: "yes please"
           · ──┬─
           ·   ╰── invalid type: string "yes please", expected either a boolean (`cors: true` / `cors: false`) or a properties map with at least an `origin` list
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        "#);
    }

    #[test]
    fn deserialize_rejects_sequence() {
        insta::assert_snapshot!(render_failure("cors: [https://example.org]\n"), @"
        martin::config::yaml (https://maplibre.org/martin/config-file/)

          × invalid type: sequence, expected either a boolean (`cors: true` / `cors:
          │ false`) or a properties map with at least an `origin` list
           ╭─[config.yaml:1:1]
         1 │ cors: [https://example.org]
           · ──┬─
           ·   ╰── invalid type: sequence, expected either a boolean (`cors: true` / `cors: false`) or a properties map with at least an `origin` list
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        ");
    }

    // ----- Existing behavior tests (default values, validation, middleware) -----

    #[test]
    fn test_cors_config_default() {
        let config = CorsConfig::default();
        let middleware = config.make_cors_middleware();
        assert!(middleware.is_some());

        // Check if it's using the default SimpleFlag(true)
        if let CorsConfig::SimpleFlag(enabled) = config {
            assert!(enabled);
        } else {
            panic!("Expected SimpleFlag variant for default config");
        }
    }

    #[test]
    fn test_cors_properties_default_values() {
        let default_props = CorsProperties::default();
        assert_eq!(default_props.origin.value, vec!["*"]);
        assert_eq!(default_props.max_age, None);
        default_props.validate(None).unwrap();
    }

    #[test]
    fn test_cors_middleware_disabled() {
        let config = CorsConfig::SimpleFlag(false);
        assert!(config.make_cors_middleware().is_none());
    }

    #[test]
    fn test_cors_yaml_parsing() {
        let config: CorsConfig = serde_saphyr::from_str(indoc! {"
            origin:
              - https://example.org
            max_age: 3600
        "})
        .unwrap();

        if let CorsConfig::Properties(settings) = config {
            assert_eq!(
                settings.origin.value,
                vec!["https://example.org".to_string()]
            );
            assert_eq!(settings.max_age, Some(3600));
        } else {
            panic!("Expected Settings variant for detailed config");
        }

        let config: CorsConfig = serde_saphyr::from_str("false").unwrap();
        assert_eq!(config, CorsConfig::SimpleFlag(false));

        let config: CorsConfig = serde_saphyr::from_str("true").unwrap();
        assert_eq!(config, CorsConfig::SimpleFlag(true));

        let config: CorsConfig = serde_saphyr::from_str(indoc! {"
            origin:
              - https://example.org
              - https://martin.maplibre.org
            max_age: 3600
        "})
        .unwrap();

        if let CorsConfig::Properties(settings) = config {
            assert_eq!(
                settings.origin.value,
                vec![
                    "https://example.org".to_string(),
                    "https://martin.maplibre.org".to_string(),
                ]
            );
            assert_eq!(settings.max_age, Some(3600));
        } else {
            panic!("Expected Settings variant for detailed config");
        }
    }

    #[test]
    fn test_cors_validation() {
        let config: CorsConfig = serde_saphyr::from_str(indoc! {"max_age: 3600"}).unwrap();
        if let CorsConfig::Properties(settings) = config {
            // This should fail validation
            assert!(matches!(
                settings.validate(None),
                Err(ConfigFileError::CorsNoOriginsConfigured(_))
            ));
        } else {
            panic!("Expected Properties variant");
        }

        let config: CorsConfig = serde_saphyr::from_str(indoc! {"
            origin:
              - https://example.org
            max_age: 3600"})
        .unwrap();

        let CorsConfig::Properties(settings) = config else {
            panic!("Expected Properties variant");
        };
        settings.validate(None).unwrap();
    }

    #[test]
    fn test_cors_validation_error_empty_origin() {
        let properties = CorsProperties {
            origin: default_spanned_empty_vec(),
            max_age: Some(3600),
            unrecognized: UnrecognizedValues::default(),
        };

        assert!(matches!(
            properties.validate(None),
            Err(ConfigFileError::CorsNoOriginsConfigured(_))
        ));
    }

    #[test]
    fn test_cors_with_valid_properties() {
        let properties = CorsProperties {
            origin: Spanned::new(
                vec!["https://example.org".to_string()],
                serde_saphyr::Location::UNKNOWN,
                serde_saphyr::Location::UNKNOWN,
            ),
            max_age: Some(3600),
            unrecognized: UnrecognizedValues::default(),
        };
        properties.validate(None).unwrap();

        let config = CorsConfig::Properties(properties);
        let middleware = config.make_cors_middleware();
        assert!(middleware.is_some());
    }

    #[test]
    fn test_cors_with_wildcard_origin() {
        let properties = CorsProperties::default();
        assert_eq!(properties.origin.value, vec!["*".to_string()]);
        properties.validate(None).unwrap();

        let middleware = CorsConfig::Properties(properties).make_cors_middleware();
        assert!(middleware.is_some());
    }
}

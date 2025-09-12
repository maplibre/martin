use actix_http::Method;
use log::info;
use serde::{Deserialize, Serialize};

use crate::config::file::{ConfigFileError, ConfigFileResult, UnrecognizedKeys, UnrecognizedValues};
use crate::{MartinError, MartinResult};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum CorsConfig {
    Properties(CorsProperties),
    SimpleFlag(bool),
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self::SimpleFlag(true)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CorsProperties {
    #[serde(default)]
    pub origin: Vec<String>,
    pub max_age: Option<usize>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl Default for CorsProperties {
    fn default() -> Self {
        Self {
            origin: vec!["*".to_string()],
            max_age: None,
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

impl ConfigExtras for CorsProperties {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}

impl CorsProperties {
    pub fn validate(&self) -> ConfigFileResult<()> {
        if self.origin.is_empty() {
            Err(ConfigFileError::CorsNoOriginsConfigured)
        } else {
            Ok(())
        }
    }
}

impl CorsConfig {
    /// Log the current configuration
    pub fn log_current_configuration(&self) {
        match &self {
            CorsConfig::SimpleFlag(false) => info!("CORS is disabled"),
            CorsConfig::SimpleFlag(true) => info!(
                "CORS enabled with defaults: {:?}",
                CorsProperties::default()
            ),
            CorsConfig::Properties(props) => {
                info!("CORS enabled with custom properties: {props:?}");
            }
        }
    }

    /// Checks that that if cors is configured explicitely (instead of via `true`/`false`), `origin` is configured
    pub fn validate(&self) -> MartinResult<()> {
        match self {
            CorsConfig::SimpleFlag(_) => Ok(()),
            CorsConfig::Properties(properties) => properties.validate().map_err(MartinError::from),
        }
    }

    #[must_use]
    /// Create [`actix_cors::Cors`] from the configuration
    pub fn make_cors_middleware(&self) -> Option<actix_cors::Cors> {
        match self {
            CorsConfig::SimpleFlag(false) => None,
            CorsConfig::SimpleFlag(true) => {
                let properties = CorsProperties::default();
                Some(Self::create_cors(&properties))
            }
            CorsConfig::Properties(properties) => Some(Self::create_cors(properties)),
        }
    }

    fn create_cors(properties: &CorsProperties) -> actix_cors::Cors {
        let mut cors = actix_cors::Cors::default();

        // allow any origin by default
        // this returns the value of the requests `ORIGIN` header in `Access-Control-Allow-Origin`
        if properties.origin.contains(&"*".to_string()) {
            cors = cors.allow_any_origin();
        } else {
            for origin in &properties.origin {
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
    use std::collections::HashMap;

    use indoc::indoc;

    use super::*;

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
        assert_eq!(default_props.origin, vec!["*"]);
        assert_eq!(default_props.max_age, None);
        assert!(default_props.validate().is_ok());
    }

    #[test]
    fn test_cors_middleware_disabled() {
        let config = CorsConfig::SimpleFlag(false);
        assert!(config.make_cors_middleware().is_none());
    }

    #[test]
    fn test_cors_yaml_parsing() {
        let config: CorsConfig = serde_yaml::from_str(indoc! {"
            origin:
              - https://example.org
            max_age: 3600
        "})
        .unwrap();

        if let CorsConfig::Properties(settings) = config {
            assert_eq!(settings.origin, vec!["https://example.org".to_string()]);
            assert_eq!(settings.max_age, Some(3600));
        } else {
            panic!("Expected Settings variant for detailed config");
        }

        let config: CorsConfig = serde_yaml::from_str("false").unwrap();
        assert_eq!(config, CorsConfig::SimpleFlag(false));

        let config: CorsConfig = serde_yaml::from_str("true").unwrap();
        assert_eq!(config, CorsConfig::SimpleFlag(true));

        let config: CorsConfig = serde_yaml::from_str(indoc! {"
            origin:
              - https://example.org
              - https://martin.maplibre.org
            max_age: 3600
        "})
        .unwrap();

        if let CorsConfig::Properties(settings) = config {
            assert_eq!(
                settings.origin,
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
        let config: CorsConfig = serde_yaml::from_str(indoc! {"max_age: 3600"}).unwrap();
        if let CorsConfig::Properties(settings) = config {
            // This should fail validation
            assert!(matches!(
                settings.validate(),
                Err(ConfigFileError::CorsNoOriginsConfigured)
            ));
        } else {
            panic!("Expected Properties variant");
        }

        let config: CorsConfig = serde_yaml::from_str(indoc! {"
            origin:
              - https://example.org
            max_age: 3600"})
        .unwrap();

        let CorsConfig::Properties(settings) = config else {
            panic!("Expected Properties variant");
        };
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn test_cors_validation_error_empty_origin() {
        let properties = CorsProperties {
            origin: vec![],
            max_age: Some(3600),
            unrecognized: HashMap::default(),
        };

        assert!(matches!(
            properties.validate(),
            Err(ConfigFileError::CorsNoOriginsConfigured)
        ));
    }

    #[test]
    fn test_cors_with_valid_properties() {
        let properties = CorsProperties {
            origin: vec!["https://example.org".to_string()],
            max_age: Some(3600),
            unrecognized: HashMap::default(),
        };
        assert!(properties.validate().is_ok());

        let config = CorsConfig::Properties(properties);
        let middleware = config.make_cors_middleware();
        assert!(middleware.is_some());
    }

    #[test]
    fn test_cors_with_wildcard_origin() {
        let properties = CorsProperties::default();
        assert_eq!(properties.origin, vec!["*".to_string()]);
        assert!(properties.validate().is_ok());

        let middleware = CorsConfig::Properties(properties).make_cors_middleware();
        assert!(middleware.is_some());
    }
}

use actix_http::Method;
use log::info;
use serde::{Deserialize, Serialize};

use crate::{MartinError, MartinResult};

#[derive(thiserror::Error, Debug)]
pub enum CorsError {
    #[error("Invalid CORS configuration")]
    PropertyValidationError(String),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum CorsConfig {
    Properties(CorsProperties),
    SimpleFlag(bool),
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self::Properties(CorsProperties::default())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CorsProperties {
    #[serde(default)]
    pub origin: Vec<String>,
    pub max_age: Option<usize>,
}

impl Default for CorsProperties {
    fn default() -> Self {
        Self {
            origin: vec!["*".to_string()],
            max_age: None,
        }
    }
}

impl CorsProperties {
    pub fn validate(&self) -> Result<(), CorsError> {
        if self.origin.is_empty() {
            Err(CorsError::PropertyValidationError(
                "When configuring CORS properties, 'origin' must be explicitly specified"
                    .to_string(),
            ))
        } else {
            Ok(())
        }
    }
}

impl CorsConfig {
    pub fn make_cors_middleware(&self) -> MartinResult<Option<actix_cors::Cors>> {
        match self {
            CorsConfig::SimpleFlag(false) => {
                info!("CORS is disabled");
                Ok(None)
            }
            CorsConfig::SimpleFlag(true) => {
                // log out the default properties / serializte to string
                info!(
                    "CORS is enabled with default properties: {:?}",
                    CorsProperties::default()
                );
                Ok(Some(Self::create_cors(&CorsProperties::default())))
            }
            CorsConfig::Properties(properties) => match properties.validate() {
                Ok(_) => Ok(Some(Self::create_cors(properties))),
                Err(e) => Err(MartinError::from(e)),
            },
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
    use indoc::indoc;

    use super::*;

    #[test]
    fn test_cors_config_default() {
        let config = CorsConfig::default();
        let middleware = config.make_cors_middleware();
        assert!(middleware.is_ok_and(|x| x.is_some()));

        // Check if it's using the appropiate default properties
        if let CorsConfig::Properties(properties) = config {
            assert_eq!(properties.origin, vec!["*"]);
            assert_eq!(properties.max_age, None);
        } else {
            panic!("Expected Properties variant for default config");
        }
    }

    #[test]
    fn test_cors_middleware_disabled() {
        let config = CorsConfig::SimpleFlag(false);
        assert_eq!(config.make_cors_middleware(), Ok(None));
    }

    #[test]
    fn test_cors_yaml_parsing() {
        // Test parsing a detailed configuration
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
        assert!(matches!(config, CorsConfig::SimpleFlag(false)));

        let config: CorsConfig = serde_yaml::from_str("true").unwrap();
        assert!(matches!(config, CorsConfig::SimpleFlag(true)));

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
        // Test parsing a config with only max_age (should fail validation)
        let config: CorsConfig = serde_yaml::from_str(indoc! {"
            max_age: 3600
        "})
        .unwrap();

        if let CorsConfig::Properties(settings) = config {
            // This should fail validation
            assert!(settings.validate().is_err());
        } else {
            panic!("Expected Properties variant");
        }

        // Test parsing a complete config (should pass validation)
        let config: CorsConfig = serde_yaml::from_str(indoc! {"
            origin:
              - https://example.org
            max_age: 3600
        "})
        .unwrap();

        if let CorsConfig::Properties(settings) = config {
            // This should pass validation
            assert!(settings.validate().is_ok());
        } else {
            panic!("Expected Properties variant");
        }
    }

    #[test]
    fn test_cors_validation_error_empty_origin() {
        // Create a CorsProperties with empty origin (should fail validation)
        let properties = CorsProperties {
            origin: vec![],
            max_age: Some(3600),
        };

        // Try to validate it - should return an error
        let validation_result = properties.validate();
        assert!(validation_result.is_err());

        // Check that the error is the right type
        match validation_result.unwrap_err() {
            CorsError::PropertyValidationError(msg) => {
                assert!(msg.contains("origin"));
            }
        }
    }

    #[test]
    fn test_cors_middleware_error_propagation() {
        // Create a config with empty origin (invalid)
        let invalid_config = CorsConfig::Properties(CorsProperties {
            origin: vec![],
            max_age: Some(3600),
        });

        // The middleware creation should propagate the validation error
        let result = invalid_config.make_cors_middleware();
        assert!(result.is_err());

        // Check that the error gets properly converted to a MartinError
        match result.unwrap_err() {
            MartinError::CorsError(CorsError::PropertyValidationError(msg)) => {
                assert!(msg.contains("origin"));
            }
            _ => panic!("Expected CorsError variant"),
        }
    }

    #[test]
    fn test_cors_with_valid_properties() {
        // Create a CorsProperties with a valid configuration
        let properties = CorsProperties {
            origin: vec!["https://example.com".to_string()],
            max_age: Some(3600),
        };

        // Validate it - should succeed
        assert!(properties.validate().is_ok());

        // Try creating middleware
        let config = CorsConfig::Properties(properties);
        let middleware = config.make_cors_middleware();
        assert!(middleware.is_ok());
        assert!(middleware.unwrap().is_some());
    }

    #[test]
    fn test_cors_with_wildcard_origin() {
        // Create a CorsProperties with wildcard origin
        let properties = CorsProperties::default();
        assert_eq!(properties.origin, vec!["*".to_string()]);

        // Validation should succeed with wildcard
        assert!(properties.validate().is_ok());

        // Creating middleware should succeed
        let middleware = CorsConfig::Properties(properties).make_cors_middleware();
        assert!(middleware.is_ok());
    }
}

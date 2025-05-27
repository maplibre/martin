use actix_http::Method;
use log::info;
use serde::{Deserialize, Serialize};

use crate::{MartinError, MartinResult};

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum CorsError {
    #[error("At least one 'origin' must be specified in the 'cors' configuration")]
    NoOriginsConfigured,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
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
            Err(CorsError::NoOriginsConfigured)
        } else {
            Ok(())
        }
    }
}

impl CorsConfig {
    pub fn validate(&self) -> MartinResult<()> {
        match self {
            CorsConfig::SimpleFlag(_) => Ok(()),
            CorsConfig::Properties(properties) => properties.validate().map_err(MartinError::from),
        }
    }

    pub fn make_cors_middleware(&self) -> MartinResult<Option<actix_cors::Cors>> {
        match self {
            CorsConfig::SimpleFlag(false) => {
                info!("CORS is disabled");
                Ok(None)
            }
            CorsConfig::SimpleFlag(true) => {
                let properties = CorsProperties::default();
                info!("Enabled CORS with defaults: {properties:?}");
                Ok(Some(Self::create_cors(&properties)))
            }
            CorsConfig::Properties(properties) => match properties.validate() {
                Ok(()) => Ok(Some(Self::create_cors(properties))),
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
        assert!(config.make_cors_middleware().is_ok_and(|m| m.is_none()));
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
            assert_eq!(settings.validate(), Err(CorsError::NoOriginsConfigured));
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
        };

        assert_eq!(properties.validate(), Err(CorsError::NoOriginsConfigured));
    }

    #[test]
    fn test_cors_middleware_error_propagation() {
        let invalid_config = CorsConfig::Properties(CorsProperties {
            origin: vec![],
            max_age: Some(3600),
        });

        let properties = invalid_config.make_cors_middleware().unwrap_err();
        assert_eq!(
            properties.to_string(),
            "At least one 'origin' must be specified in the 'cors' configuration".to_string()
        );
    }

    #[test]
    fn test_cors_with_valid_properties() {
        let properties = CorsProperties {
            origin: vec!["https://example.com".to_string()],
            max_age: Some(3600),
        };
        assert!(properties.validate().is_ok());

        let config = CorsConfig::Properties(properties);
        let middleware = config.make_cors_middleware();
        assert!(middleware.unwrap().is_some());
    }

    #[test]
    fn test_cors_with_wildcard_origin() {
        let properties = CorsProperties::default();
        assert_eq!(properties.origin, vec!["*".to_string()]);
        assert!(properties.validate().is_ok());

        let middleware = CorsConfig::Properties(properties).make_cors_middleware();
        assert!(middleware.is_ok());
    }
}

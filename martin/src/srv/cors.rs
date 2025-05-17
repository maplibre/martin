use crate::MartinResult;
use actix_http::Method;
use serde::{Deserialize, Serialize};

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
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.origin.is_empty() {
            Err(
                "When configuring CORS properties, 'origin' must be explicitly specified (e.g., origin: ['*'] for allowing any origin)",
            )
        } else {
            Ok(())
        }
    }
}

impl CorsConfig {
    pub fn make_cors_middleware(&self) -> Option<actix_cors::Cors> {
        match self {
            CorsConfig::SimpleFlag(false) => None,
            CorsConfig::SimpleFlag(true) => Some(Self::create_cors(&CorsProperties::default())),
            CorsConfig::Properties(properties) => match properties.validate() {
                Ok(_) => Some(Self::create_cors(properties)),
                Err(e) => {
                    println!("yo");
                    None
                }
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
        assert!(middleware.is_some());

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
        assert!(config.make_cors_middleware().is_none());
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
}

use actix_http::Method;
use serde::{Deserialize, Serialize};

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct CorsConfig {
    pub enable: bool,
    pub allowed_origins: Vec<String>,
    pub max_age: Option<usize>,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            enable: true,
            allowed_origins: vec!["*".to_string()],
            max_age: None,
        }
    }
}

impl CorsConfig {
    pub fn make_cors_middleware(&self) -> Option<actix_cors::Cors> {
        if self.enable {
            // start with the recommended restrictive library defaults
            let mut cors = actix_cors::Cors::default();

            // allow any origin by default
            // this will set `access-control-allow-origin` dynamically to the value of the `Origin` request header
            if self.allowed_origins.contains(&"*".to_string()) {
                cors = cors.allow_any_origin();
            }
            // if specific origins are provided, set them instead
            else {
                for origin in &self.allowed_origins {
                    cors = cors.allowed_origin(origin);
                }
            }

            // only allow GET method by default
            cors = cors.allowed_methods([Method::GET]);

            // sets `Access-Control-Max-Age` if configured
            cors = cors.max_age(self.max_age);

            Some(cors)
        } else {
            None
        }
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
        assert!(config.enable);
        assert_eq!(config.allowed_origins, vec!["*"]);
        assert_eq!(config.max_age, None);
    }

    #[test]
    fn test_cors_middleware_disabled() {
        let config = CorsConfig {
            enable: false,
            ..Default::default()
        };
        assert!(config.make_cors_middleware().is_none());
    }

    #[test]
    fn test_cors_yaml_parsing() {
        let config: CorsConfig = serde_yaml::from_str(indoc! {"
            enable: true
            allowed_origins: ['https://example.com']
            max_age: 3600
        "})
        .unwrap();
        assert_eq!(
            config,
            CorsConfig {
                enable: true,
                allowed_origins: vec!["https://example.com".to_string()],
                max_age: Some(3600),
            }
        );

        let config: CorsConfig = serde_yaml::from_str(indoc! {"
            enable: false
        "})
        .unwrap();
        assert_eq!(
            config,
            CorsConfig {
                enable: false,
                ..Default::default()
            }
        );

        let config: CorsConfig = serde_yaml::from_str(indoc! {"
            allowed_origins: ['https://example1.com', 'https://example2.com']
        "})
        .unwrap();
        assert_eq!(
            config,
            CorsConfig {
                allowed_origins: vec![
                    "https://example1.com".to_string(),
                    "https://example2.com".to_string(),
                ],
                ..Default::default()
            }
        );
    }
}

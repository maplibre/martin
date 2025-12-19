#[cfg(feature = "metrics")]
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::config::args::PreferredEncoding;
#[cfg(feature = "metrics")]
use crate::config::file::UnrecognizedValues;
use crate::config::file::cors::CorsConfig;
use crate::config::file::{ConfigurationLivecycleHooks, UnrecognizedKeys};

pub const KEEP_ALIVE_DEFAULT: u64 = 75;
pub const LISTEN_ADDRESSES_DEFAULT: &str = "0.0.0.0:3000";

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct SrvConfig {
    pub keep_alive: Option<u64>,
    pub listen_addresses: Option<String>,
    pub base_path: Option<String>,
    pub worker_processes: Option<usize>,
    pub preferred_encoding: Option<PreferredEncoding>,
    #[cfg(all(feature = "webui", not(docsrs)))]
    pub web_ui: Option<crate::config::args::WebUiMode>,
    pub cors: Option<CorsConfig>,
    /// Advanced monitoring options
    #[cfg(feature = "metrics")]
    pub observability: Option<ObservabilityConfig>,
    #[cfg(feature = "_tiles")]
    pub tilejson_url_version_param: Option<String>,
}

impl ConfigurationLivecycleHooks for SrvConfig {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        let mut unrecognized = UnrecognizedKeys::new();
        if let Some(CorsConfig::Properties(cors)) = &self.cors {
            unrecognized.extend(
                cors.get_unrecognized_keys()
                    .iter()
                    .map(|k| format!("cors.{k}")),
            );
        }
        #[cfg(feature = "metrics")]
        if let Some(observability) = &self.observability {
            unrecognized.extend(
                observability
                    .get_unrecognized_keys()
                    .iter()
                    .map(|k| format!("observability.{k}")),
            );
        }
        unrecognized
    }
}

/// More advanced monitoring montoring options
#[cfg(feature = "metrics")]
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct ObservabilityConfig {
    /// Configure metrics reported under `/_/metrics`
    pub metrics: Option<MetricsConfig>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

#[cfg(feature = "metrics")]
impl ConfigurationLivecycleHooks for ObservabilityConfig {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        let mut keys = self
            .unrecognized
            .keys()
            .cloned()
            .collect::<UnrecognizedKeys>();
        if let Some(metrics) = &self.metrics {
            keys.extend(
                metrics
                    .get_unrecognized_keys()
                    .iter()
                    .map(|k| format!("metrics.{k}")),
            );
        }
        keys
    }
}

/// Configure metrics reported under `/_/metrics`
#[cfg(feature = "metrics")]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct MetricsConfig {
    /// Add these labels to every metric
    ///
    /// # Example:
    /// ```json
    /// { env: prod, server: martin }
    /// ```
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub add_labels: HashMap<String, String>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

#[cfg(feature = "metrics")]
impl ConfigurationLivecycleHooks for MetricsConfig {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;
    use crate::config::file::UnrecognizedValues;
    use crate::config::file::cors::CorsProperties;

    #[test]
    fn parse_config() {
        assert_eq!(
            serde_yaml::from_str::<SrvConfig>(indoc! {"
                keep_alive: 75
                listen_addresses: '0.0.0.0:3000'
                worker_processes: 8
            "})
            .unwrap(),
            SrvConfig {
                keep_alive: Some(75),
                listen_addresses: Some("0.0.0.0:3000".to_string()),
                worker_processes: Some(8),
                ..Default::default()
            }
        );
        assert_eq!(
            serde_yaml::from_str::<SrvConfig>(indoc! {"
                keep_alive: 75
                listen_addresses: '0.0.0.0:3000'
                worker_processes: 8
                preferred_encoding: br
            "})
            .unwrap(),
            SrvConfig {
                keep_alive: Some(75),
                listen_addresses: Some("0.0.0.0:3000".to_string()),
                worker_processes: Some(8),
                preferred_encoding: Some(PreferredEncoding::Brotli),
                ..Default::default()
            }
        );
        assert_eq!(
            serde_yaml::from_str::<SrvConfig>(indoc! {"
                keep_alive: 75
                listen_addresses: '0.0.0.0:3000'
                worker_processes: 8
                preferred_encoding: brotli
            "})
            .unwrap(),
            SrvConfig {
                keep_alive: Some(75),
                listen_addresses: Some("0.0.0.0:3000".to_string()),
                worker_processes: Some(8),
                preferred_encoding: Some(PreferredEncoding::Brotli),
                ..Default::default()
            }
        );
    }

    #[test]
    fn parse_config_cors() {
        assert_eq!(
            serde_yaml::from_str::<SrvConfig>(indoc! {"
                keep_alive: 75
                listen_addresses: '0.0.0.0:3000'
                worker_processes: 8
                cors: false
            "})
            .unwrap(),
            SrvConfig {
                keep_alive: Some(75),
                listen_addresses: Some("0.0.0.0:3000".to_string()),
                worker_processes: Some(8),
                cors: Some(CorsConfig::SimpleFlag(false)),
                ..Default::default()
            }
        );
        assert_eq!(
            serde_yaml::from_str::<SrvConfig>(indoc! {"
                keep_alive: 75
                listen_addresses: '0.0.0.0:3000'
                worker_processes: 8
                cors: true
            "})
            .unwrap(),
            SrvConfig {
                keep_alive: Some(75),
                listen_addresses: Some("0.0.0.0:3000".to_string()),
                worker_processes: Some(8),
                cors: Some(CorsConfig::SimpleFlag(true)),
                ..Default::default()
            }
        );
        assert_eq!(
            serde_yaml::from_str::<SrvConfig>(indoc! {"
                keep_alive: 75
                listen_addresses: '0.0.0.0:3000'
                worker_processes: 8
                cors:
                  origin:
                    - https://martin.maplibre.org
                    - https://example.org
            "})
            .unwrap(),
            SrvConfig {
                keep_alive: Some(75),
                listen_addresses: Some("0.0.0.0:3000".to_string()),
                worker_processes: Some(8),
                cors: Some(CorsConfig::Properties(CorsProperties {
                    origin: vec![
                        "https://martin.maplibre.org".to_string(),
                        "https://example.org".to_string()
                    ],
                    max_age: None,
                    unrecognized: UnrecognizedValues::default()
                })),
                ..Default::default()
            }
        );
    }
}

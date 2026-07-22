#[cfg(feature = "metrics")]
use std::collections::HashMap;
use std::fmt;

use actix_web::http::header::{CacheDirective, HeaderValue, from_comma_delimited};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de, ser};

use crate::config::args::PreferredEncoding;
#[cfg(all(feature = "webui", not(docsrs)))]
use crate::config::args::WebUiMode;
#[cfg(feature = "metrics")]
use crate::config::file::UnrecognizedValues;
use crate::config::file::cors::CorsConfig;
use crate::config::file::{CollectUnrecognizedKeys, ConfigurationLivecycleHooks, UnrecognizedKeys};

pub const DEFAULT_KEEP_ALIVE: u64 = 75;
pub const DEFAULT_LISTEN_ADDRESSES: &str = "0.0.0.0:3000";

/// A syntactically and semantically validated `Cache-Control` header value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheControlHeader(HeaderValue);

impl CollectUnrecognizedKeys for CacheControlHeader {
    fn collect_unrecognized(&self, _path: &str, _out: &mut UnrecognizedKeys) {}
}

impl CacheControlHeader {
    #[must_use]
    pub(crate) fn header_value(&self) -> HeaderValue {
        self.0.clone()
    }
}

impl fmt::Display for CacheControlHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.to_str().map_err(|_| fmt::Error)?)
    }
}

impl Serialize for CacheControlHeader {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0.to_str().map_err(ser::Error::custom)?)
    }
}

impl<'de> Deserialize<'de> for CacheControlHeader {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        let value = HeaderValue::from_str(&raw).map_err(de::Error::custom)?;
        let directives: Vec<CacheDirective> = from_comma_delimited(std::iter::once(&value))
            .map_err(|error| {
                de::Error::custom(format_args!(
                    "invalid Cache-Control header value '{raw}': {error}"
                ))
            })?;
        if directives.is_empty() {
            return Err(de::Error::custom(format_args!(
                "invalid Cache-Control header value '{raw}': no valid directives"
            )));
        }
        Ok(Self(value))
    }
}

#[serde_with::skip_serializing_none]
#[derive(
    Clone,
    Debug,
    Serialize,
    Deserialize,
    PartialEq,
    Default,
    CollectUnrecognizedKeys,
    ConfigurationLivecycleHooks,
)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct SrvConfig {
    /// Connection keep alive timeout \[default: 75\]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &75u64))]
    pub keep_alive: Option<u64>,
    /// The socket address to bind \[default: `0.0.0.0:3000`\]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"0.0.0.0:3000"))]
    pub listen_addresses: Option<String>,
    /// Set the URL path prefix for all API routes.
    /// When set, Martin will serve all endpoints under this path prefix.
    /// This allows Martin to be served under a subpath when behind a reverse proxy (e.g., Traefik).
    /// Must begin with a `/`.
    /// Examples: `/tiles`, `/api/v1/tiles`
    pub route_prefix: Option<String>,
    /// Set `TileJSON` URL path prefix.
    /// This overrides the default path prefix for URLs in `TileJSON` responses.
    /// If both `route_prefix` and `base_path` are set, `base_path` takes priority for `TileJSON` URLs.
    /// If neither is set, the `X-Rewrite-URL` header is respected.
    /// Must begin with a `/`.
    /// Examples: `/`, `/tiles`
    pub base_path: Option<String>,
    /// Number of web server workers
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &8usize))]
    pub worker_processes: Option<usize>,
    /// Which compression should be used if the
    /// - client accepts multiple compression formats, and
    /// - tile source is not pre-compressed.
    ///
    /// `gzip` is faster, but `brotli` is smaller, and may be faster with caching.
    /// Default could be different depending on Martin version.
    pub preferred_encoding: Option<PreferredEncoding>,
    /// Set the default `Cache-Control` response header.
    ///
    /// The value is used for responses that do not define a more specific cache policy. For
    /// example: `public, max-age=3600`. Endpoints with an explicit policy, such as the health
    /// check, keep their own header.
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "Option<String>"))]
    pub cache_control: Option<CacheControlHeader>,
    /// Enable or disable Martin web UI. \[default: disable\]
    ///
    /// At the moment, only allows `enable-for-all`, which enables the web UI for all connections.
    /// This may be undesirable in a production environment
    #[cfg(all(feature = "webui", not(docsrs)))]
    pub web_ui: Option<WebUiMode>,
    /// CORS Configuration
    ///
    /// Defaults to `cors: true`, which allows all origins.
    /// Sending/Acting on CORS headers can be completely disabled via `cors: false`
    pub cors: Option<CorsConfig>,
    /// Advanced monitoring options
    #[cfg(feature = "metrics")]
    pub observability: Option<ObservabilityConfig>,
    /// If set, the version of the tileset (as specified in the `MBTiles` or `PMTiles` metadata)
    /// will be embedded in the `TileJSON` `tiles` URL, with the set identifier.
    /// This is useful to give clients a better way to cache-bust a CDN:
    /// 1. maplibre requests tilejson, tilejson contains the tiles URL. This is always up-to-date.
    /// 2. maplibre requests each tile it requires, with the tiles URL in the tilejson.
    /// 3. Add `Control: public, max-age=..., immutable` on the tile responses
    ///    optimize browser/CDN cache hit rates, while also making sure that
    ///    old tiles aren't served when a new tileset is deployed.
    ///
    /// The CDN must handle query parameters for caching to work correctly.
    /// Many CDNs ignore them by default.
    ///
    /// For example, if
    /// - the setting here is `version`, and
    /// - the `PMTiles` tileset version is `1.0.0`, the
    /// `TileJSON` will be:
    /// `{ ..., "tiles": [".../{z}/{x}/{y}?version=1.0.0"], ... }`
    #[cfg(feature = "_tiles")]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"version"))]
    pub tilejson_url_version_param: Option<String>,
}

impl SrvConfig {
    pub(crate) fn cache_control_header(&self) -> Option<HeaderValue> {
        self.cache_control
            .as_ref()
            .map(CacheControlHeader::header_value)
    }

    /// The URL path prefix under which Martin is publicly served, derived from
    /// the explicit config (not request headers).
    ///
    /// Returns `base_path` if set, otherwise `route_prefix`, otherwise `None`.
    /// The two have different deployment semantics (see field docs) but both
    /// describe the public-facing prefix and so are interchangeable for the
    /// purpose of building absolute URLs in responses.
    #[must_use]
    pub fn public_path_prefix(&self) -> Option<&str> {
        self.base_path.as_deref().or(self.route_prefix.as_deref())
    }
}

/// More advanced monitoring options
#[cfg(feature = "metrics")]
#[serde_with::skip_serializing_none]
#[derive(
    Clone,
    Debug,
    Serialize,
    Deserialize,
    PartialEq,
    Default,
    CollectUnrecognizedKeys,
    ConfigurationLivecycleHooks,
)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct ObservabilityConfig {
    /// Configure metrics reported under `/_/metrics`
    pub metrics: Option<MetricsConfig>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

/// Configure metrics reported under `/_/metrics`
#[cfg(feature = "metrics")]
#[derive(
    Clone,
    Debug,
    Serialize,
    Deserialize,
    PartialEq,
    Default,
    CollectUnrecognizedKeys,
    ConfigurationLivecycleHooks,
)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct MetricsConfig {
    /// Add these labels to every metric
    /// Example: `{ env: prod, server: martin }`
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub add_labels: HashMap<String, String>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;
    use crate::config::file::UnrecognizedValues;
    use crate::config::file::cors::CorsProperties;
    use crate::config::test_helpers::render_failure;

    #[test]
    fn parse_config() {
        assert_eq!(
            serde_saphyr::from_str::<SrvConfig>(indoc! {"
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
            serde_saphyr::from_str::<SrvConfig>(indoc! {"
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
            serde_saphyr::from_str::<SrvConfig>(indoc! {"
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
    fn parse_cache_control() {
        let config = serde_saphyr::from_str::<SrvConfig>(indoc! {"
            cache_control: public, max-age=3600, stale-while-revalidate=60
        "})
        .unwrap();

        assert_eq!(
            config.cache_control.as_ref().map(ToString::to_string),
            Some("public, max-age=3600, stale-while-revalidate=60".to_string())
        );
        assert_eq!(
            config.cache_control_header().unwrap(),
            "public, max-age=3600, stale-while-revalidate=60"
        );
    }

    #[test]
    fn reject_invalid_cache_control_header() {
        insta::assert_snapshot!(
            render_failure(indoc::indoc! {"
                cache_control: max-age=invalid
            "}),
            @"");
    }

    #[test]
    fn reject_empty_cache_control_header() {
        insta::assert_snapshot!(
            render_failure(indoc::indoc! {"
                cache_control: ""
            "}),
            @"");
    }

    #[test]
    fn parse_config_cors() {
        assert_eq!(
            serde_saphyr::from_str::<SrvConfig>(indoc! {"
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
            serde_saphyr::from_str::<SrvConfig>(indoc! {"
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
            serde_saphyr::from_str::<SrvConfig>(indoc! {"
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

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use martin_core::tiles::BoxedSource;
use martin_core::tiles::passthrough::{PassthroughSource, TemplateMeta, Transport, Upstream};
use martin_tile_utils::Format;
use serde::de::value::{MapAccessDeserializer, SeqAccessDeserializer};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use tilejson::Bounds;
use tracing::info;

use crate::MartinResult;
use crate::config::file::{
    CachePolicy, ConfigFileError, ConfigurationLivecycleHooks, ResolutionResult, TileSourceWarning,
    UnrecognizedKeys, UnrecognizedValues,
};
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::{MltProcessConfig, MvtProcessConfig};
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::primitives::AutoOption;
use crate::config::primitives::{IdResolver, OptOneMany};

/// Default per-request timeout for upstream fetches.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

fn default_timeout() -> Duration {
    DEFAULT_TIMEOUT
}

fn is_default_timeout(timeout: &Duration) -> bool {
    *timeout == DEFAULT_TIMEOUT
}

/// A worked `sources` map for the generated config docs, showing the shorthand,
/// `TileJSON`, and detailed-object forms side by side.
#[cfg(feature = "unstable-schemas")]
fn passthrough_sources_example() -> serde_json::Value {
    serde_json::json!({
        "osm": "https://tile.openstreetmap.org/{z}/{x}/{y}.png",
        "hosted": "https://demotiles.maplibre.org/tiles/tiles.json",
        "secure": {
            "url": "https://api.example.com/{z}/{x}/{y}",
            "headers": { "Authorization": "${API_TOKEN}" },
            "format": "mvt",
            "minzoom": 0,
            "maxzoom": 14
        }
    })
}

/// Configuration for the `passthrough` source type: a sources map plus type-level
/// MVT<->MLT conversion defaults. Unlike file sources there are no `paths:` to glob.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct PassthroughConfig {
    /// MVT->MLT encoder settings for all passthrough sources.
    /// Overrides global; overridden by per-source `convert_to_mlt`.
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[serde(default)]
    pub convert_to_mlt: Option<MltProcessConfig>,

    /// MLT->MVT conversion settings for all passthrough sources.
    /// Overrides global; overridden by per-source `convert_to_mvt`.
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[serde(default)]
    pub convert_to_mvt: Option<MvtProcessConfig>,

    /// Upstream tile servers to proxy, keyed by the source ID Martin serves them under.
    ///
    /// Each value is one of:
    /// - a `{z}/{x}/{y}` URL template, e.g. `https://tile.openstreetmap.org/{z}/{x}/{y}.png`
    /// - a `TileJSON` document URL; its tile URLs, zoom range, and bounds are read from the document
    /// - a list of URL templates, to spread requests across mirror upstreams
    /// - an object with `url` plus any of `headers` (e.g. for auth), `timeout`, `format`,
    ///   `minzoom`/`maxzoom`/`bounds`/`attribution`, `cache`, and `convert_to_mlt`/`convert_to_mvt`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &passthrough_sources_example()))]
    pub sources: Option<BTreeMap<String, PassthroughSrc>>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl PassthroughConfig {
    /// Returns `true` if no sources and no custom settings are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let empty = self.sources.as_ref().is_none_or(BTreeMap::is_empty)
            && self.get_unrecognized_keys().is_empty();
        #[cfg(all(feature = "mlt", feature = "_tiles"))]
        let empty = empty && self.convert_to_mlt.is_none() && self.convert_to_mvt.is_none();
        empty
    }

    /// Resolve every configured source into a [`BoxedSource`], collecting per-source failures as
    /// [`TileSourceWarning`]s so one bad upstream does not abort the others.
    ///
    /// The `sources` map is rewritten so its keys become the [`IdResolver`]-assigned source ids,
    /// matching what [`build_process_config_map`](crate::config::file::Config) later keys on.
    pub async fn resolve(
        &mut self,
        idr: &IdResolver,
        default_cache: CachePolicy,
    ) -> ResolutionResult {
        let mut results = Vec::new();
        let mut warnings = Vec::new();

        if let Some(sources) = self.sources.take() {
            let mut resolved = BTreeMap::new();
            for (id, src) in sources {
                let cfg = src.to_config();
                let dedup_key = cfg
                    .url
                    .as_slice()
                    .first()
                    .cloned()
                    .unwrap_or_else(|| id.clone());
                let id = idr.resolve(&id, dedup_key);
                match cfg.build(id.clone(), default_cache).await {
                    Ok(source) => {
                        info!(source.id = %id, "Configured passthrough source");
                        results.push(source);
                        resolved.insert(id, src);
                    }
                    Err(error) => warnings.push(TileSourceWarning::SourceError {
                        source_id: id,
                        error: error.to_string(),
                    }),
                }
            }
            self.sources = Some(resolved);
        }

        Ok((results, warnings))
    }
}

impl ConfigurationLivecycleHooks for PassthroughConfig {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        let mut keys: UnrecognizedKeys = self.unrecognized.keys().cloned().collect();
        if let Some(sources) = &self.sources {
            for (id, src) in sources {
                let PassthroughSrc::Detailed(obj) = src else {
                    continue;
                };
                keys.extend(obj.unrecognized.keys().map(|k| format!("sources.{id}.{k}")));
                #[cfg(all(feature = "mlt", feature = "_tiles"))]
                {
                    if let Some(AutoOption::Explicit(cfg)) = obj.convert_to_mlt.as_ref() {
                        keys.extend(
                            cfg.unrecognized_keys()
                                .map(|k| format!("sources.{id}.convert_to_mlt.{k}")),
                        );
                    }
                    if let Some(AutoOption::Explicit(cfg)) = obj.convert_to_mvt.as_ref() {
                        keys.extend(
                            cfg.unrecognized_keys()
                                .map(|k| format!("sources.{id}.convert_to_mvt.{k}")),
                        );
                    }
                }
            }
        }
        keys
    }
}

/// A passthrough source value: either a bare upstream URL (or list of URLs) or a full
/// configuration object.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum PassthroughSrc {
    /// Shorthand: an upstream URL template, a `TileJSON` URL, or a list of URL templates.
    Shorthand(OptOneMany<String>),
    /// A configuration object with headers, timeout, format, and metadata.
    /// Boxed because it is much larger than the shorthand variant.
    Detailed(Box<PassthroughSourceConfig>),
}

impl PassthroughSrc {
    /// Normalize either form into a [`PassthroughSourceConfig`].
    #[must_use]
    fn to_config(&self) -> PassthroughSourceConfig {
        match self {
            Self::Shorthand(url) => PassthroughSourceConfig {
                url: url.clone(),
                ..PassthroughSourceConfig::default()
            },
            Self::Detailed(cfg) => (**cfg).clone(),
        }
    }
}

impl<'de> Deserialize<'de> for PassthroughSrc {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PassthroughSrcVisitor;

        impl<'de> Visitor<'de> for PassthroughSrcVisitor {
            type Value = PassthroughSrc;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    "an upstream URL string, a list of URL strings, or a configuration map with a \
                     `url` field",
                )
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<PassthroughSrc, E> {
                Ok(PassthroughSrc::Shorthand(OptOneMany::One(
                    value.to_string(),
                )))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<PassthroughSrc, E> {
                Ok(PassthroughSrc::Shorthand(OptOneMany::One(value)))
            }

            fn visit_seq<S: SeqAccess<'de>>(self, seq: S) -> Result<PassthroughSrc, S::Error> {
                let urls: Vec<String> = Deserialize::deserialize(SeqAccessDeserializer::new(seq))?;
                Ok(PassthroughSrc::Shorthand(OptOneMany::new(urls)))
            }

            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<PassthroughSrc, M::Error> {
                let obj = PassthroughSourceConfig::deserialize(MapAccessDeserializer::new(map))?;
                Ok(PassthroughSrc::Detailed(Box::new(obj)))
            }
        }

        deserializer.deserialize_any(PassthroughSrcVisitor)
    }
}

/// Per-source passthrough configuration object.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct PassthroughSourceConfig {
    /// Upstream tile-URL template(s) (`{z}/{x}/{y}`) or a single `TileJSON` document URL.
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub url: OptOneMany<String>,

    /// HTTP headers sent with every upstream request (e.g. `Authorization`).
    /// Values support `${ENV_VAR}` substitution via the config loader.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,

    /// Per-request timeout. Supports human-readable formats: "30s", "1m". Defaults to "30s".
    #[serde(
        default = "default_timeout",
        with = "humantime_serde",
        skip_serializing_if = "is_default_timeout"
    )]
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "String", example = &"30s"))]
    pub timeout: Duration,

    /// Explicit tile format override (e.g. `mvt`, `png`). When unset, the format is detected
    /// from the URL extension, the upstream `TileJSON`, or the response.
    pub format: Option<String>,

    /// Minimum zoom level advertised in the served `TileJSON` (template sources only).
    pub minzoom: Option<u8>,
    /// Maximum zoom level advertised in the served `TileJSON` (template sources only).
    pub maxzoom: Option<u8>,
    /// Geographic bounds advertised in the served `TileJSON` (template sources only).
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "Option<[f64; 4]>"))]
    pub bounds: Option<Bounds>,
    /// Attribution advertised in the served `TileJSON` (template sources only).
    pub attribution: Option<String>,

    /// Zoom-level bounds for tile caching.
    #[serde(default, skip_serializing_if = "CachePolicy::is_empty")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "crate::config::file::CachePolicyShape")
    )]
    pub cache: CachePolicy,

    /// MVT->MLT encoder settings for this source.
    /// Overrides source-type and global `convert_to_mlt`.
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[serde(default)]
    pub convert_to_mlt: Option<MltProcessConfig>,
    /// MLT->MVT conversion settings for this source.
    /// Overrides source-type and global `convert_to_mvt`.
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[serde(default)]
    pub convert_to_mvt: Option<MvtProcessConfig>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl Default for PassthroughSourceConfig {
    fn default() -> Self {
        Self {
            url: OptOneMany::default(),
            headers: BTreeMap::default(),
            timeout: DEFAULT_TIMEOUT,
            format: None,
            minzoom: None,
            maxzoom: None,
            bounds: None,
            attribution: None,
            cache: CachePolicy::default(),
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            convert_to_mlt: None,
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            convert_to_mvt: None,
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

impl PassthroughSourceConfig {
    /// Build the upstream into a live [`BoxedSource`], fetching the upstream `TileJSON` once for a
    /// document upstream.
    async fn build(&self, id: String, default_cache: CachePolicy) -> MartinResult<BoxedSource> {
        let format = match self.format.as_deref() {
            Some(value) => Some(Format::parse(value).ok_or_else(|| {
                ConfigFileError::InvalidPassthroughFormat {
                    source_id: id.clone(),
                    tile_format: value.to_string(),
                }
            })?),
            None => None,
        };
        let meta = TemplateMeta {
            minzoom: self.minzoom,
            maxzoom: self.maxzoom,
            bounds: self.bounds,
            attribution: self.attribution.clone(),
        };
        let urls = self.url.as_slice().to_vec();
        let upstream = Upstream::from_config(&id, &urls, format, meta)?;
        let transport = Transport::from_string_headers(
            self.timeout,
            self.headers.iter().map(|(k, v)| (k.as_str(), v.as_str())),
        )?;
        let cache = self.cache.or(default_cache);
        let source = PassthroughSource::new(id, upstream, transport, cache.zoom()).await?;
        Ok(Box::new(source))
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    fn parse(yaml: &str) -> PassthroughConfig {
        serde_saphyr::from_str(yaml).expect("parses")
    }

    #[test]
    fn shorthand_string_source() {
        let cfg = parse(indoc! {"
            sources:
              osm: https://tiles.example.com/{z}/{x}/{y}.pbf
        "});
        let src = &cfg.sources.as_ref().unwrap()["osm"];
        assert_eq!(
            src,
            &PassthroughSrc::Shorthand(OptOneMany::One(
                "https://tiles.example.com/{z}/{x}/{y}.pbf".to_string()
            ))
        );
    }

    #[test]
    fn shorthand_list_source() {
        let cfg = parse(indoc! {"
            sources:
              osm:
                - https://a.example.com/{z}/{x}/{y}.pbf
                - https://b.example.com/{z}/{x}/{y}.pbf
        "});
        let src = &cfg.sources.as_ref().unwrap()["osm"];
        let PassthroughSrc::Shorthand(OptOneMany::Many(urls)) = src else {
            panic!("expected a list shorthand, got {src:?}");
        };
        assert_eq!(urls.len(), 2);
    }

    #[test]
    fn detailed_object_source() {
        let cfg = parse(indoc! {"
            sources:
              secure:
                url: https://api.example.com/v1/{z}/{x}/{y}.mvt
                headers:
                  Authorization: Bearer token
                timeout: 45s
                format: mvt
                minzoom: 0
                maxzoom: 14
                bounds: [-180, -85, 180, 85]
        "});
        let src = &cfg.sources.as_ref().unwrap()["secure"];
        let PassthroughSrc::Detailed(obj) = src else {
            panic!("expected a detailed object, got {src:?}");
        };
        assert_eq!(
            obj.url,
            OptOneMany::One("https://api.example.com/v1/{z}/{x}/{y}.mvt".to_string())
        );
        assert_eq!(obj.headers["Authorization"], "Bearer token");
        assert_eq!(obj.timeout, Duration::from_secs(45));
        assert_eq!(obj.format.as_deref(), Some("mvt"));
        assert_eq!(obj.minzoom, Some(0));
        assert_eq!(obj.maxzoom, Some(14));
        assert!(obj.bounds.is_some());
    }

    #[test]
    fn default_timeout_is_30s() {
        let cfg = parse(indoc! {"
            sources:
              s:
                url: https://e.example.com/{z}/{x}/{y}.pbf
        "});
        let PassthroughSrc::Detailed(obj) = &cfg.sources.as_ref().unwrap()["s"] else {
            panic!("expected detailed");
        };
        assert_eq!(obj.timeout, Duration::from_secs(30));
    }

    #[test]
    fn unrecognized_per_source_key_is_reported() {
        let cfg = parse(indoc! {"
            sources:
              s:
                url: https://e.example.com/{z}/{x}/{y}.pbf
                typoo: 1
        "});
        let keys = cfg.get_unrecognized_keys();
        assert!(
            keys.contains("sources.s.typoo"),
            "expected sources.s.typoo in {keys:?}"
        );
    }

    #[test]
    fn empty_config_is_empty() {
        assert!(PassthroughConfig::default().is_empty());
        let cfg = parse(indoc! {"
            sources:
              s: https://e.example.com/{z}/{x}/{y}.pbf
        "});
        assert!(!cfg.is_empty());
    }
}

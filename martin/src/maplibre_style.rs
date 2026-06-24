//! Partial [`MapLibre` style spec][spec] types.
//!
//! Only the fields needed for URL rewriting are modeled; everything else
//! round-trips through `serde_json::Value` via `#[serde(flatten)]`.
//!
//! This module is intentionally self-contained and depends only on
//! `serde` / `serde_json`, so it could be lifted into a standalone crate later.
//!
//! [spec]: https://maplibre.org/maplibre-style-spec/

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

/// A partially-typed `MapLibre` style document.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Style {
    pub glyphs: Option<String>,

    pub sprite: Option<Sprite>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, Source>,

    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

/// The `sprite` field is either a single URL or a list of `{id, url}` entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Sprite {
    Single(String),
    Multi(Vec<SpriteEntry>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpriteEntry {
    pub id: String,
    pub url: String,
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub url: Option<String>,

    pub tiles: Option<Vec<String>>,

    /// May be a URL string (for remote `GeoJSON`) or inline `GeoJSON`.
    pub data: Option<Value>,

    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

impl Style {
    /// Rewrite any URL field that lacks a scheme (does not contain `://`)
    /// by prepending `base_url`.
    ///
    /// Lets a style.json on disk use protocol-less URLs like
    /// `"/font/{fontstack}/{range}"`, which the `MapLibre` style spec doesn't
    /// allow, while still serving spec-compliant absolute URLs to clients.
    ///
    /// Fields rewritten: top-level `glyphs`, `sprite` (both string and
    /// `[{id, url}]` forms), and per-source `url`, `tiles[]`, and `data`
    /// (only when `data` is a string).
    pub fn expand_relative_urls(&mut self, base_url: &str) {
        if let Some(glyphs) = &mut self.glyphs {
            expand_if_relative_url(glyphs, base_url);
        }
        if let Some(sprite) = &mut self.sprite {
            sprite.expand_relative_urls(base_url);
        }
        for source in self.sources.values_mut() {
            source.expand_relative_urls(base_url);
        }
    }
}

impl Sprite {
    fn expand_relative_urls(&mut self, base_url: &str) {
        match self {
            Self::Single(url) => expand_if_relative_url(url, base_url),
            Self::Multi(entries) => {
                for entry in entries {
                    expand_if_relative_url(&mut entry.url, base_url);
                }
            }
        }
    }
}

impl Source {
    fn expand_relative_urls(&mut self, base_url: &str) {
        if let Some(url) = &mut self.url {
            expand_if_relative_url(url, base_url);
        }
        if let Some(tiles) = &mut self.tiles {
            for t in tiles {
                expand_if_relative_url(t, base_url);
            }
        }
        if let Some(Value::String(url)) = &mut self.data {
            expand_if_relative_url(url, base_url);
        }
    }
}

fn expand_if_relative_url(url: &mut String, base_url: &str) {
    // Protocol-relative URL like `//cdn.example/x` -> leave alone.
    if url.starts_with("//") {
        return;
    }
    // Already a valid absolute URL -> leave alone.
    if Url::parse(url).is_ok() {
        return;
    }
    // Ensure exactly one '/' between the base and the path, so a relative path
    // without a leading slash (e.g. `fonts/{fontstack}`) doesn't get glued onto
    // the prefix to produce `https://host/prefixfonts/...`.
    if !url.starts_with('/') {
        url.insert(0, '/');
    }
    url.insert_str(0, base_url);
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parse(v: Value) -> Style {
        serde_json::from_value(v).unwrap()
    }

    fn dump(s: &Style) -> Value {
        serde_json::to_value(s).unwrap()
    }

    #[test]
    fn leaves_absolute_urls_alone() {
        let original = json!({
            "version": 8,
            "glyphs": "https://example.com/font/{fontstack}/{range}.pbf",
            "sprite": "https://example.com/sprite",
            "sources": {
                "v": {
                    "type": "vector",
                    "url": "https://example.com/tiles.json",
                    "tiles": ["https://example.com/{z}/{x}/{y}.pbf"]
                },
                "g": {
                    "type": "geojson",
                    "data": "https://example.com/things.geojson"
                }
            }
        });
        let mut style = parse(original.clone());
        style.expand_relative_urls("https://martin.example");
        assert_eq!(dump(&style), original);
    }

    #[test]
    fn rewrites_relative_urls() {
        let mut style = parse(json!({
            "version": 8,
            "glyphs": "/font/{fontstack}/{range}.pbf",
            "sprite": "/sprite/my_sprite",
            "sources": {
                "v": {
                    "type": "vector",
                    "url": "/my_source",
                    "tiles": ["/my_source/{z}/{x}/{y}"]
                },
                "g": {
                    "type": "geojson",
                    "data": "/things.geojson"
                }
            }
        }));
        style.expand_relative_urls("https://martin.example");
        assert_eq!(
            dump(&style),
            json!({
                "version": 8,
                "glyphs": "https://martin.example/font/{fontstack}/{range}.pbf",
                "sprite": "https://martin.example/sprite/my_sprite",
                "sources": {
                    "v": {
                        "type": "vector",
                        "url": "https://martin.example/my_source",
                        "tiles": ["https://martin.example/my_source/{z}/{x}/{y}"]
                    },
                    "g": {
                        "type": "geojson",
                        "data": "https://martin.example/things.geojson"
                    }
                }
            })
        );
    }

    #[test]
    fn rewrites_sprite_array_form() {
        let mut style = parse(json!({
            "sprite": [
                {"id": "default", "url": "/sprite/main"},
                {"id": "other", "url": "https://cdn.example/sprite/other"}
            ]
        }));
        style.expand_relative_urls("https://martin.example");
        assert_eq!(
            dump(&style),
            json!({
                "sprite": [
                    {"id": "default", "url": "https://martin.example/sprite/main"},
                    {"id": "other", "url": "https://cdn.example/sprite/other"}
                ]
            })
        );
    }

    #[test]
    fn leaves_inline_geojson_data_alone() {
        let original = json!({
            "sources": {
                "g": {
                    "type": "geojson",
                    "data": {"type": "FeatureCollection", "features": []}
                }
            }
        });
        let mut style = parse(original.clone());
        style.expand_relative_urls("https://martin.example");
        assert_eq!(dump(&style), original);
    }

    #[test]
    fn leaves_non_http_schemes_alone() {
        let original = json!({
            "sources": {
                "m": {"type": "vector", "url": "mbtiles://my.mbtiles"},
                "p": {"type": "vector", "url": "pmtiles://my.pmtiles"}
            }
        });
        let mut style = parse(original.clone());
        style.expand_relative_urls("https://martin.example");
        assert_eq!(dump(&style), original);
    }

    #[test]
    fn only_updates_specified_fields() {
        let mut style = parse(json!({
            "sprite": "/sprite/touch_this",
            "not_sprite": "/sprite/dont_touch_this",
        }));
        style.expand_relative_urls("https://martin.example");
        assert_eq!(
            dump(&style),
            json!({
                "sprite": "https://martin.example/sprite/touch_this",
                "not_sprite": "/sprite/dont_touch_this",
            })
        );
    }

    fn expanded(url: &str, base: &str) -> String {
        let mut s = url.to_string();
        expand_if_relative_url(&mut s, base);
        s
    }

    #[test]
    fn expand_leaves_http_schemes_alone() {
        assert_eq!(
            expanded("https://example.com/x", "https://martin.example"),
            "https://example.com/x"
        );
        assert_eq!(
            expanded("http://example.com/x", "https://martin.example"),
            "http://example.com/x"
        );
    }

    #[test]
    fn expand_leaves_custom_schemes_alone() {
        assert_eq!(
            expanded("mapbox://styles/foo", "https://martin.example"),
            "mapbox://styles/foo"
        );
        assert_eq!(
            expanded("mbtiles://my.mbtiles", "https://martin.example"),
            "mbtiles://my.mbtiles"
        );
    }

    #[test]
    fn expand_leaves_data_and_mailto_uris_alone() {
        assert_eq!(
            expanded("data:font/ttf;base64,AAAA", "https://martin.example"),
            "data:font/ttf;base64,AAAA"
        );
        assert_eq!(
            expanded("mailto:nobody@example.com", "https://martin.example"),
            "mailto:nobody@example.com"
        );
    }

    #[test]
    fn expand_leaves_protocol_relative_urls_alone() {
        assert_eq!(
            expanded("//cdn.example/sprite", "https://martin.example"),
            "//cdn.example/sprite"
        );
    }

    #[test]
    fn expand_prepends_base_to_path_absolute_url() {
        assert_eq!(
            expanded("/font/{fontstack}", "https://martin.example/prefix"),
            "https://martin.example/prefix/font/{fontstack}"
        );
    }

    #[test]
    fn expand_joins_relative_path_with_single_slash() {
        assert_eq!(
            expanded("fonts/{fontstack}", "https://martin.example/prefix"),
            "https://martin.example/prefix/fonts/{fontstack}"
        );
        assert_eq!(
            expanded("fonts/{fontstack}", "https://martin.example"),
            "https://martin.example/fonts/{fontstack}"
        );
    }

    #[test]
    fn expand_does_not_treat_colon_in_path_segment_as_scheme() {
        // A path segment containing ':' after a non-alpha first char isn't a scheme.
        assert_eq!(
            expanded("1bad:scheme/foo", "https://martin.example"),
            "https://martin.example/1bad:scheme/foo"
        );
    }

    #[test]
    fn preserves_unknown_fields() {
        let original = json!({
            "version": 8,
            "name": "demo",
            "metadata": {"author": "me"},
            "layers": [{"id": "background", "type": "background"}],
            "center": [0.0, 0.0],
            "zoom": 3
        });
        let style = parse(original.clone());
        assert_eq!(dump(&style), original);
    }
}

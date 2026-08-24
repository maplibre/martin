//! Partial [`MapLibre` style spec][spec] types.
//!
//! Only the fields needed for URL rewriting are modeled; everything else
//! round-trips through `serde_json::Value` via `#[serde(flatten)]`.
//!
//! This module is intentionally self-contained and depends only on
//! `serde` / `serde_json`, so it could be lifted into a standalone crate later.
//!
//! [spec]: https://maplibre.org/maplibre-style-spec/

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

/// A partially-typed `MapLibre` style document.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Style {
    pub glyphs: Option<String>,

    pub sprite: Option<Sprite>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, Source>,

    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

/// The `sprite` field is either a single URL or a list of `{id, url}` entries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Sprite {
    Single(String),
    Multi(Vec<SpriteEntry>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpriteEntry {
    pub id: String,
    pub url: String,
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Source {
    pub url: Option<String>,

    pub tiles: Option<Vec<String>>,

    /// May be a URL string (for remote `GeoJSON`) or inline `GeoJSON`.
    pub data: Option<Value>,

    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

/// An error encountered while combining multiple `MapLibre` style documents.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum StyleMergeError {
    #[error("At least one style is required")]
    NoStyles,

    #[error("Style {style_id:?} has no layers array")]
    MissingLayers { style_id: String },

    #[error("Style {style_id:?} has a layers field that is not an array")]
    InvalidLayers { style_id: String },

    #[error("Layer {index} in style {style_id:?} is not an object")]
    InvalidLayer { style_id: String, index: usize },

    #[error("Layer {index} in style {style_id:?} has no string id")]
    InvalidLayerId { style_id: String, index: usize },

    #[error("Layer {layer_id:?} in style {style_id:?} has a source that is not a string")]
    InvalidLayerSource { style_id: String, layer_id: String },

    #[error(
        "Cannot merge styles {first_style:?} and {second_style:?}: source {source_id:?} has different definitions"
    )]
    SourceConflict {
        first_style: String,
        second_style: String,
        source_id: String,
    },

    #[error(
        "Cannot merge styles {first_style:?} and {second_style:?}: layer id {layer_id:?} exists in both styles"
    )]
    LayerConflict {
        first_style: String,
        second_style: String,
        layer_id: String,
    },

    #[error("Cannot merge styles {first_style:?} and {second_style:?}: glyph URLs are different")]
    GlyphConflict {
        first_style: String,
        second_style: String,
    },

    #[error("Style {style_id:?} has a font-faces field that is not an object")]
    InvalidFontFaces { style_id: String },

    #[error(
        "Cannot merge styles {first_style:?} and {second_style:?}: font face {font_face:?} has different definitions"
    )]
    FontFaceConflict {
        first_style: String,
        second_style: String,
        font_face: String,
    },

    #[error("Style {style_id:?} has a state field that is not an object")]
    InvalidState { style_id: String },

    #[error(
        "Cannot merge styles {first_style:?} and {second_style:?}: state property {state_property:?} has different definitions"
    )]
    StateConflict {
        first_style: String,
        second_style: String,
        state_property: String,
    },

    #[error(
        "Cannot merge styles {first_style:?} and {second_style:?}: sprite definitions are incompatible"
    )]
    SpriteConflict {
        first_style: String,
        second_style: String,
    },

    #[error(
        "Cannot merge styles {first_style:?} and {second_style:?}: sprite id {sprite_id:?} has different definitions"
    )]
    SpriteIdConflict {
        first_style: String,
        second_style: String,
        sprite_id: String,
    },
}

/// Merge styles in request order, keeping the first style's root properties.
///
/// The `font-faces` and `state` root maps are merged by key. Layer arrays are
/// concatenated, structurally identical sources are de-duplicated, and layers
/// are rewritten to use the first name under which a source definition
/// appeared. Conflicts that cannot be represented without changing style
/// semantics are rejected.
pub fn merge_styles(
    styles: Vec<(String, Style)>,
    martin_base_url: &str,
) -> Result<Style, StyleMergeError> {
    let mut styles = styles.into_iter();
    let Some((first_id, mut first_style)) = styles.next() else {
        return Err(StyleMergeError::NoStyles);
    };

    let first_layers = first_style.other.remove("layers");
    let first_font_faces = first_style.other.remove("font-faces");
    let first_state = first_style.other.remove("state");
    let mut result = Style {
        other: std::mem::take(&mut first_style.other),
        ..Style::default()
    };
    if let Some(first_layers) = first_layers {
        first_style.other.insert("layers".to_owned(), first_layers);
    }
    if let Some(first_font_faces) = first_font_faces {
        first_style
            .other
            .insert("font-faces".to_owned(), first_font_faces);
    }
    if let Some(first_state) = first_state {
        first_style.other.insert("state".to_owned(), first_state);
    }
    let mut source_names = Vec::new();
    let mut seen_sources = HashMap::new();
    let mut layer_owners = HashMap::new();
    let mut font_face_owners = HashMap::new();
    let mut state_owners = HashMap::new();
    let mut layers = Vec::new();
    let mut glyph_owner = None;
    let mut sprite_owner = None;

    for (style_id, mut style) in std::iter::once((first_id, first_style)).chain(styles) {
        merge_glyphs(
            &mut result.glyphs,
            &mut glyph_owner,
            style.glyphs.take(),
            &style_id,
        )?;
        merge_sprites(
            &mut result.sprite,
            &mut sprite_owner,
            style.sprite.take(),
            &style_id,
            martin_base_url,
        )?;
        merge_root_map(
            &mut result.other,
            &mut font_face_owners,
            &mut style.other,
            &style_id,
            RootMap::FontFaces,
        )?;
        merge_root_map(
            &mut result.other,
            &mut state_owners,
            &mut style.other,
            &style_id,
            RootMap::State,
        )?;

        let aliases = merge_sources(
            &mut result.sources,
            &mut source_names,
            &mut seen_sources,
            std::mem::take(&mut style.sources),
            &style_id,
        )?;
        merge_layers(
            &mut layers,
            &mut layer_owners,
            &aliases,
            &mut style.other,
            &style_id,
        )?;
    }

    result
        .other
        .insert("layers".to_owned(), Value::Array(layers));
    Ok(result)
}

#[derive(Clone, Copy)]
enum RootMap {
    FontFaces,
    State,
}

impl RootMap {
    const fn field(self) -> &'static str {
        match self {
            Self::FontFaces => "font-faces",
            Self::State => "state",
        }
    }

    fn invalid(self, style_id: &str) -> StyleMergeError {
        match self {
            Self::FontFaces => StyleMergeError::InvalidFontFaces {
                style_id: style_id.to_owned(),
            },
            Self::State => StyleMergeError::InvalidState {
                style_id: style_id.to_owned(),
            },
        }
    }

    fn conflict(self, first_style: String, second_style: &str, key: String) -> StyleMergeError {
        match self {
            Self::FontFaces => StyleMergeError::FontFaceConflict {
                first_style,
                second_style: second_style.to_owned(),
                font_face: key,
            },
            Self::State => StyleMergeError::StateConflict {
                first_style,
                second_style: second_style.to_owned(),
                state_property: key,
            },
        }
    }
}

fn merge_root_map(
    merged: &mut BTreeMap<String, Value>,
    owners: &mut HashMap<String, String>,
    style_other: &mut BTreeMap<String, Value>,
    style_id: &str,
    kind: RootMap,
) -> Result<(), StyleMergeError> {
    let Some(value) = style_other.remove(kind.field()) else {
        return Ok(());
    };
    let Value::Object(values) = value else {
        return Err(kind.invalid(style_id));
    };

    let merged_values = merged
        .entry(kind.field().to_owned())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .expect("merged root maps are always JSON objects");
    for (key, value) in values {
        if let Some(existing) = merged_values.get(&key) {
            if existing != &value {
                return Err(kind.conflict(
                    owners
                        .get(&key)
                        .cloned()
                        .expect("a merged root map entry always has an owner"),
                    style_id,
                    key,
                ));
            }
        } else {
            owners.insert(key.clone(), style_id.to_owned());
            merged_values.insert(key, value);
        }
    }
    Ok(())
}

fn merge_glyphs(
    merged: &mut Option<String>,
    owner: &mut Option<String>,
    glyphs: Option<String>,
    style_id: &str,
) -> Result<(), StyleMergeError> {
    let Some(glyphs) = glyphs else {
        return Ok(());
    };
    match merged {
        None => {
            *merged = Some(glyphs);
            *owner = Some(style_id.to_owned());
            Ok(())
        }
        Some(existing) if existing == &glyphs => Ok(()),
        Some(_) => Err(StyleMergeError::GlyphConflict {
            first_style: owner
                .clone()
                .expect("a merged glyph URL always has an owner"),
            second_style: style_id.to_owned(),
        }),
    }
}

fn merge_sources(
    merged: &mut BTreeMap<String, Source>,
    canonical_names: &mut Vec<String>,
    seen: &mut HashMap<String, (Source, String)>,
    sources: BTreeMap<String, Source>,
    style_id: &str,
) -> Result<BTreeMap<String, String>, StyleMergeError> {
    let mut aliases = BTreeMap::new();
    for (source_id, source) in sources {
        if let Some((existing, first_style)) = seen.get(&source_id) {
            if existing != &source {
                return Err(StyleMergeError::SourceConflict {
                    first_style: first_style.clone(),
                    second_style: style_id.to_owned(),
                    source_id,
                });
            }
        } else {
            seen.insert(source_id.clone(), (source.clone(), style_id.to_owned()));
        }

        let canonical = canonical_names
            .iter()
            .find(|name| merged.get(*name) == Some(&source))
            .cloned();
        if let Some(canonical) = canonical {
            aliases.insert(source_id, canonical);
        } else {
            aliases.insert(source_id.clone(), source_id.clone());
            canonical_names.push(source_id.clone());
            merged.insert(source_id, source);
        }
    }
    Ok(aliases)
}

fn merge_layers(
    merged: &mut Vec<Value>,
    owners: &mut HashMap<String, String>,
    aliases: &BTreeMap<String, String>,
    other: &mut BTreeMap<String, Value>,
    style_id: &str,
) -> Result<(), StyleMergeError> {
    let layers = match other.remove("layers") {
        Some(Value::Array(layers)) => layers,
        Some(_) => {
            return Err(StyleMergeError::InvalidLayers {
                style_id: style_id.to_owned(),
            });
        }
        None => {
            return Err(StyleMergeError::MissingLayers {
                style_id: style_id.to_owned(),
            });
        }
    };

    for (index, mut layer) in layers.into_iter().enumerate() {
        let Some(object) = layer.as_object_mut() else {
            return Err(StyleMergeError::InvalidLayer {
                style_id: style_id.to_owned(),
                index,
            });
        };
        let Some(layer_id) = object.get("id").and_then(Value::as_str).map(str::to_owned) else {
            return Err(StyleMergeError::InvalidLayerId {
                style_id: style_id.to_owned(),
                index,
            });
        };

        if let Some(source) = object.get_mut("source") {
            let Some(source_id) = source.as_str() else {
                return Err(StyleMergeError::InvalidLayerSource {
                    style_id: style_id.to_owned(),
                    layer_id,
                });
            };
            if let Some(canonical) = aliases.get(source_id) {
                *source = Value::String(canonical.clone());
            }
        }

        if let Some(first_style) = owners.insert(layer_id.clone(), style_id.to_owned()) {
            return Err(StyleMergeError::LayerConflict {
                first_style,
                second_style: style_id.to_owned(),
                layer_id,
            });
        }
        merged.push(layer);
    }
    Ok(())
}

fn merge_sprites(
    merged: &mut Option<Sprite>,
    owner: &mut Option<String>,
    sprite: Option<Sprite>,
    style_id: &str,
    martin_base_url: &str,
) -> Result<(), StyleMergeError> {
    let Some(sprite) = sprite else {
        return Ok(());
    };
    let Some(existing) = merged else {
        *merged = Some(sprite);
        *owner = Some(style_id.to_owned());
        return Ok(());
    };
    if existing == &sprite {
        return Ok(());
    }

    let first_style = owner
        .clone()
        .expect("a merged sprite definition always has an owner");
    match (existing, sprite) {
        (Sprite::Single(first), Sprite::Single(second)) => {
            let Some(composite) = composite_sprite_url(first, &second, martin_base_url) else {
                return Err(StyleMergeError::SpriteConflict {
                    first_style,
                    second_style: style_id.to_owned(),
                });
            };
            *first = composite;
            Ok(())
        }
        (Sprite::Multi(first), Sprite::Multi(second)) => {
            for entry in second {
                if let Some(existing) = first.iter().find(|item| item.id == entry.id) {
                    if existing != &entry {
                        return Err(StyleMergeError::SpriteIdConflict {
                            first_style,
                            second_style: style_id.to_owned(),
                            sprite_id: entry.id,
                        });
                    }
                } else {
                    first.push(entry);
                }
            }
            Ok(())
        }
        (Sprite::Single(_), Sprite::Multi(_)) | (Sprite::Multi(_), Sprite::Single(_)) => {
            Err(StyleMergeError::SpriteConflict {
                first_style,
                second_style: style_id.to_owned(),
            })
        }
    }
}

fn composite_sprite_url(first: &str, second: &str, martin_base_url: &str) -> Option<String> {
    fn split<'a>(url: &'a str, sprite_prefix: &str) -> Option<Vec<&'a str>> {
        let ids = url.strip_prefix(sprite_prefix)?;
        if ids.is_empty() || ids.contains('/') || ids.contains('?') || ids.contains('#') {
            return None;
        }
        Some(ids.split(',').collect())
    }

    let sprite_prefix = format!("{}/sprite/", martin_base_url.trim_end_matches('/'));
    let mut first_ids = split(first, &sprite_prefix)?;
    let second_ids = split(second, &sprite_prefix)?;
    for id in second_ids {
        if !first_ids.contains(&id) {
            first_ids.push(id);
        }
    }
    Some(format!("{sprite_prefix}{}", first_ids.join(",")))
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

    const MARTIN_BASE_URL: &str = "https://martin.test/prefix";

    fn parse(v: Value) -> Style {
        serde_json::from_value(v).unwrap()
    }

    fn dump(s: &Style) -> Value {
        serde_json::to_value(s).unwrap()
    }

    fn merge(styles: Vec<(String, Style)>) -> Result<Style, StyleMergeError> {
        merge_styles(styles, MARTIN_BASE_URL)
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
        let mut s = url.to_owned();
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

    #[test]
    fn merges_layers_and_rewrites_structurally_identical_source_aliases() {
        let base = parse(json!({
            "version": 8,
            "name": "base",
            "metadata": {"keep": true},
            "glyphs": "https://example.com/fonts/{fontstack}/{range}.pbf",
            "font-faces": {
                "Base Font": "https://fonts.example/base.ttf"
            },
            "state": {
                "base-visible": {"default": true}
            },
            "sources": {
                "canonical": {"type": "vector", "url": "https://example.com/tiles.json"}
            },
            "layers": [{"id": "base-layer", "type": "fill", "source": "canonical"}]
        }));
        let overlay = parse(json!({
            "version": 7,
            "name": "overlay",
            "metadata": {"discard": true},
            "glyphs": "https://example.com/fonts/{fontstack}/{range}.pbf",
            "font-faces": {
                "Overlay Font": "https://fonts.example/overlay.ttf"
            },
            "state": {
                "overlay-opacity": {"default": 0.5}
            },
            "sources": {
                "alias": {"type": "vector", "url": "https://example.com/tiles.json"},
                "points": {"type": "geojson", "data": {"type": "FeatureCollection", "features": []}}
            },
            "layers": [
                {"id": "alias-layer", "type": "line", "source": "alias"},
                {
                    "id": "points-layer",
                    "type": "symbol",
                    "source": "points",
                    "layout": {"text-font": ["Overlay Font"]},
                    "paint": {"text-opacity": ["global-state", "overlay-opacity"]}
                }
            ]
        }));

        let merged = merge(vec![
            ("base".to_owned(), base),
            ("overlay".to_owned(), overlay),
        ])
        .unwrap();

        assert_eq!(
            dump(&merged),
            json!({
                "version": 8,
                "name": "base",
                "metadata": {"keep": true},
                "glyphs": "https://example.com/fonts/{fontstack}/{range}.pbf",
                "font-faces": {
                    "Base Font": "https://fonts.example/base.ttf",
                    "Overlay Font": "https://fonts.example/overlay.ttf"
                },
                "state": {
                    "base-visible": {"default": true},
                    "overlay-opacity": {"default": 0.5}
                },
                "sources": {
                    "canonical": {"type": "vector", "url": "https://example.com/tiles.json"},
                    "points": {"type": "geojson", "data": {"type": "FeatureCollection", "features": []}}
                },
                "layers": [
                    {"id": "base-layer", "type": "fill", "source": "canonical"},
                    {"id": "alias-layer", "type": "line", "source": "canonical"},
                    {
                        "id": "points-layer",
                        "type": "symbol",
                        "source": "points",
                        "layout": {"text-font": ["Overlay Font"]},
                        "paint": {"text-opacity": ["global-state", "overlay-opacity"]}
                    }
                ]
            })
        );
    }

    #[test]
    fn rejects_different_sources_with_the_same_name_even_after_aliasing() {
        let source = json!({"type": "vector", "url": "https://example.com/a"});
        let styles = vec![
            (
                "base".to_owned(),
                parse(json!({
                    "sources": {"canonical": source.clone()},
                    "layers": [{"id": "a", "source": "canonical"}]
                })),
            ),
            (
                "alias".to_owned(),
                parse(json!({
                    "sources": {"shared": source},
                    "layers": [{"id": "b", "source": "shared"}]
                })),
            ),
            (
                "conflict".to_owned(),
                parse(json!({
                    "sources": {"shared": {"type": "vector", "url": "https://example.com/b"}},
                    "layers": [{"id": "c", "source": "shared"}]
                })),
            ),
        ];

        assert_eq!(
            merge(styles),
            Err(StyleMergeError::SourceConflict {
                first_style: "alias".to_owned(),
                second_style: "conflict".to_owned(),
                source_id: "shared".to_owned(),
            })
        );
    }

    #[test]
    fn rejects_duplicate_layer_ids() {
        let result = merge(vec![
            (
                "base".to_owned(),
                parse(json!({"layers": [{"id": "water", "type": "fill"}]})),
            ),
            (
                "overlay".to_owned(),
                parse(json!({"layers": [{"id": "water", "type": "line"}]})),
            ),
        ]);
        assert_eq!(
            result,
            Err(StyleMergeError::LayerConflict {
                first_style: "base".to_owned(),
                second_style: "overlay".to_owned(),
                layer_id: "water".to_owned(),
            })
        );
    }

    #[test]
    fn rejects_different_glyph_templates() {
        let result = merge(vec![
            (
                "base".to_owned(),
                parse(json!({"glyphs": "https://a/{fontstack}", "layers": []})),
            ),
            (
                "overlay".to_owned(),
                parse(json!({"glyphs": "https://b/{fontstack}", "layers": []})),
            ),
        ]);
        assert_eq!(
            result,
            Err(StyleMergeError::GlyphConflict {
                first_style: "base".to_owned(),
                second_style: "overlay".to_owned(),
            })
        );
    }

    #[test]
    fn combines_martin_sprite_urls() {
        let merged = merge(vec![
            (
                "base".to_owned(),
                parse(json!({"sprite": "https://martin.test/prefix/sprite/base", "layers": []})),
            ),
            (
                "overlay".to_owned(),
                parse(json!({"sprite": "https://martin.test/prefix/sprite/poi", "layers": []})),
            ),
            (
                "again".to_owned(),
                parse(json!({"sprite": "https://martin.test/prefix/sprite/base", "layers": []})),
            ),
        ])
        .unwrap();
        assert_eq!(
            merged.sprite,
            Some(Sprite::Single(
                "https://martin.test/prefix/sprite/base,poi".to_owned()
            ))
        );
    }

    #[test]
    fn rejects_external_sprite_urls_that_resemble_martin_endpoints() {
        let result = merge(vec![
            (
                "base".to_owned(),
                parse(json!({"sprite": "https://cdn.example/sprite/base", "layers": []})),
            ),
            (
                "overlay".to_owned(),
                parse(json!({"sprite": "https://cdn.example/sprite/overlay", "layers": []})),
            ),
        ]);
        assert_eq!(
            result,
            Err(StyleMergeError::SpriteConflict {
                first_style: "base".to_owned(),
                second_style: "overlay".to_owned(),
            })
        );
    }

    #[test]
    fn rejects_conflicting_font_faces_and_state_properties() {
        let font_face_result = merge(vec![
            (
                "base".to_owned(),
                parse(json!({
                    "font-faces": {"Shared Font": "https://fonts.example/a.ttf"},
                    "layers": []
                })),
            ),
            (
                "overlay".to_owned(),
                parse(json!({
                    "font-faces": {"Shared Font": "https://fonts.example/b.ttf"},
                    "layers": []
                })),
            ),
        ]);
        assert_eq!(
            font_face_result,
            Err(StyleMergeError::FontFaceConflict {
                first_style: "base".to_owned(),
                second_style: "overlay".to_owned(),
                font_face: "Shared Font".to_owned(),
            })
        );

        let state_result = merge(vec![
            (
                "base".to_owned(),
                parse(json!({
                    "state": {"opacity": {"default": 0.5}},
                    "layers": []
                })),
            ),
            (
                "overlay".to_owned(),
                parse(json!({
                    "state": {"opacity": {"default": 1.0}},
                    "layers": []
                })),
            ),
        ]);
        assert_eq!(
            state_result,
            Err(StyleMergeError::StateConflict {
                first_style: "base".to_owned(),
                second_style: "overlay".to_owned(),
                state_property: "opacity".to_owned(),
            })
        );
    }

    #[test]
    fn merges_multi_sprite_entries_and_rejects_id_conflicts() {
        let base = parse(json!({
            "sprite": [{"id": "base", "url": "https://example.com/base"}],
            "layers": []
        }));
        let overlay = parse(json!({
            "sprite": [
                {"id": "base", "url": "https://example.com/base"},
                {"id": "poi", "url": "https://example.com/poi"}
            ],
            "layers": []
        }));
        let merged = merge(vec![
            ("base".to_owned(), base.clone()),
            ("overlay".to_owned(), overlay),
        ])
        .unwrap();
        assert_eq!(
            dump(&merged)["sprite"],
            json!([
                {"id": "base", "url": "https://example.com/base"},
                {"id": "poi", "url": "https://example.com/poi"}
            ])
        );

        let conflict = parse(json!({
            "sprite": [{"id": "base", "url": "https://example.com/different"}],
            "layers": []
        }));
        assert_eq!(
            merge(vec![
                ("base".to_owned(), base),
                ("conflict".to_owned(), conflict),
            ]),
            Err(StyleMergeError::SpriteIdConflict {
                first_style: "base".to_owned(),
                second_style: "conflict".to_owned(),
                sprite_id: "base".to_owned(),
            })
        );
    }

    #[test]
    fn validates_layers_only_when_merging() {
        assert_eq!(
            merge(vec![
                ("base".to_owned(), parse(json!({"layers": []}))),
                ("missing".to_owned(), parse(json!({}))),
            ]),
            Err(StyleMergeError::MissingLayers {
                style_id: "missing".to_owned()
            })
        );
    }
}

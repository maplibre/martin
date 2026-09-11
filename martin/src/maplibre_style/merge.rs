use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use super::{Source, Sprite, Style};

/// An error encountered while combining multiple `MapLibre` style documents.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum StyleMergeError {
    #[error("At least one style is required")]
    NoStyles,

    #[error("Style {style_id:?} cannot be merged: {what}")]
    Invalid { style_id: String, what: String },

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

    #[error(
        "Cannot merge styles {first_style:?} and {second_style:?}: font face {font_face:?} has different definitions"
    )]
    FontFaceConflict {
        first_style: String,
        second_style: String,
        font_face: String,
    },

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
        return Err(StyleMergeError::Invalid {
            style_id: style_id.to_owned(),
            what: format!("{} is not an object", kind.field()),
        });
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
    let Some(glyphs) = glyphs.filter(|value| !value.is_empty()) else {
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
            return Err(StyleMergeError::Invalid {
                style_id: style_id.to_owned(),
                what: "layers is not an array".to_owned(),
            });
        }
        None => {
            return Err(StyleMergeError::Invalid {
                style_id: style_id.to_owned(),
                what: "there is no layers array".to_owned(),
            });
        }
    };

    for (index, mut layer) in layers.into_iter().enumerate() {
        let Some(object) = layer.as_object_mut() else {
            return Err(StyleMergeError::Invalid {
                style_id: style_id.to_owned(),
                what: format!("layer {index} is not an object"),
            });
        };
        let Some(layer_id) = object.get("id").and_then(Value::as_str).map(str::to_owned) else {
            return Err(StyleMergeError::Invalid {
                style_id: style_id.to_owned(),
                what: format!("layer {index} has no string id"),
            });
        };

        if let Some(source) = object.get_mut("source") {
            let Some(source_id) = source.as_str() else {
                return Err(StyleMergeError::Invalid {
                    style_id: style_id.to_owned(),
                    what: format!("layer {layer_id:?} has a source that is not a string"),
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

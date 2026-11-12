//! The metadata `martin cp --mode bulk` writes, keeping what tile mode writes where it can.

use itertools::Itertools as _;
use martin_tile_utils::Encoding;
use tilejson::{Bounds, TileJSON, tilejson};

use crate::config::file::postgres::utils::patch_json;

/// What a bulk tileset says about itself, before the generator adds zooms, format and the `vector_layers` it
/// computed.
pub struct BulkMetadata<'a> {
    /// The tileset's name when no table comment names it: the output's file stem.
    pub name: Option<String>,
    /// The area the tiles cover.
    pub bounds: Option<Bounds>,
    /// The `compression` key, for `MBTiles` output only.
    pub compression: Option<Encoding>,
    /// The `TileJSON` of each table's SQL comment, in table order.
    pub comments: Vec<Option<&'a serde_json::Value>>,
    /// The `generator` key.
    pub generator: String,
}

impl BulkMetadata<'_> {
    /// Merges the table comments as tile mode merges its sources: attributions, descriptions and names deduplicated
    /// and joined, the first center, and `vector_layers` that win over computed ones of the same id. Other keys
    /// keep the first table's value.
    #[must_use]
    pub fn into_tilejson(self) -> TileJSON {
        let mut metadata = tilejson! { tiles: vec![] };
        let mut attributions = Vec::new();
        let mut descriptions = Vec::new();
        let mut names = Vec::new();
        for comment in self.comments.into_iter().flatten() {
            let comment = patch_json(tilejson! { tiles: vec![] }, Some(comment));
            push_new(&mut attributions, comment.attribution);
            push_new(&mut descriptions, comment.description);
            push_new(&mut names, comment.name);
            metadata.center = metadata.center.or(comment.center);
            metadata.version = metadata.version.or(comment.version);
            metadata.legend = metadata.legend.or(comment.legend);
            metadata.template = metadata.template.or(comment.template);
            if let Some(layers) = comment.vector_layers {
                metadata.vector_layers.get_or_insert_default().extend(layers);
            }
            for (key, value) in comment.other {
                metadata.other.entry(key).or_insert(value);
            }
        }
        metadata.attribution = (!attributions.is_empty()).then(|| attributions.into_iter().join("\n"));
        metadata.description = (!descriptions.is_empty()).then(|| descriptions.into_iter().join("\n"));
        metadata.name = if names.is_empty() {
            self.name
        } else {
            Some(names.into_iter().join(","))
        };
        metadata.bounds = self.bounds;
        metadata.other.insert("generator".to_owned(), self.generator.into());
        match self.compression.and_then(Encoding::compression) {
            Some(compression) => metadata.other.insert("compression".to_owned(), compression.into()),
            None => metadata.other.remove("compression"),
        };
        metadata
    }
}

/// Appends `value` to `values` unless it is missing or already there.
fn push_new(values: &mut Vec<String>, value: Option<String>) {
    if let Some(value) = value
        && !values.contains(&value)
    {
        values.push(value);
    }
}

use martin_tile_utils::Format;
use serde_json::{Map, Value, json};
use tilejson::TileJSON;

/// A raster collection drawn by a generated style.
pub(super) struct Drawn<'a> {
    /// ID of the collection, the ID of its source.
    pub id: &'a str,
    /// `TileJSON` of the collection as it is served.
    pub tilejson: &'a TileJSON,
}

/// Whether a generated style can draw tiles in `format`, which only raster images are.
pub(super) fn drawable(format: Format) -> bool {
    matches!(format, Format::Png | Format::Jpeg | Format::Webp)
}

/// A style drawing the raster tiles of `collections` over a white background, in order, from `tiles`, the URL Martin serves tiles under.
pub(super) fn generated_style(tiles: &str, collections: &[Drawn]) -> Value {
    let mut sources = Map::new();
    let mut layers = vec![json!({
        "id": "background",
        "type": "background",
        "paint": { "background-color": "#ffffff" },
    })];
    for collection in collections {
        let source = format!("source:{}", collection.id);
        let mut definition = json!({
            "type": "raster",
            "tiles": [format!("{tiles}/{}/{{z}}/{{x}}/{{y}}", super::link::path_segment(collection.id))],
            "tileSize": collection.tilejson.other.get("tileSize").and_then(Value::as_u64).unwrap_or(256),
            "minzoom": collection.tilejson.minzoom.unwrap_or(0),
            "maxzoom": collection.tilejson.maxzoom.unwrap_or(22),
        });
        if let Some(bounds) = collection.tilejson.bounds {
            definition["bounds"] = json!([bounds.left, bounds.bottom, bounds.right, bounds.top]);
        }
        layers.push(json!({ "id": source, "type": "raster", "source": source }));
        sources.insert(source, definition);
    }
    json!({ "version": 8, "sources": sources, "layers": layers })
}

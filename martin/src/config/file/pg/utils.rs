use log::error;
use tilejson::TileJSON;

#[must_use]
pub fn patch_json(target: TileJSON, patch: Option<&serde_json::Value>) -> TileJSON {
    let Some(tj) = patch else {
        // Nothing to merge in, keep the original
        return target;
    };
    // Not the most efficient, but this is only executed once per source:
    // * Convert the TileJSON struct to a serde_json::Value
    // * Merge the self.tilejson into the value
    // * Convert the merged value back to a TileJSON struct
    // * In case of errors, return the original tilejson
    let mut tilejson2 = match serde_json::to_value(target.clone()) {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to serialize tilejson, unable to merge function comment: {e}");
            return target;
        }
    };
    json_patch::merge(&mut tilejson2, tj);
    match serde_json::from_value(tilejson2.clone()) {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to deserialize merged function comment tilejson: {e}");
            target
        }
    }
}

use std::collections::HashMap;

use postgres::types::Json;

use crate::tiles::UrlQuery;

/// Converts a `UrlQuery` into a semantically identical `Json<HashMap<String, serde_json::Value>>`.
#[must_use]
pub fn query_to_json(query: Option<&UrlQuery>) -> Json<HashMap<String, serde_json::Value>> {
    let mut query_as_json = HashMap::new();
    if let Some(query) = query {
        for (k, v) in query {
            let json_value: serde_json::Value =
                serde_json::from_str(v).unwrap_or_else(|_| serde_json::Value::String(v.clone()));

            query_as_json.insert(k.clone(), json_value);
        }
    }

    Json(query_as_json)
}

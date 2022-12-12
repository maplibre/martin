use crate::source::UrlQuery;
use crate::utils::InfoMap;
use actix_http::header::HeaderValue;
use actix_web::http::Uri;
use postgis::{ewkb, LineString, Point, Polygon};
use postgres::types::Json;
use std::collections::HashMap;
use tilejson::{tilejson, Bounds, TileJSON, VectorLayer};

#[macro_export]
macro_rules! io_error {
    ($format:literal $(, $arg:expr)* $(,)?) => {
        ::std::io::Error::new(
            ::std::io::ErrorKind::Other,
            ::std::format!($format, $($arg,)*))
    };
    ($error:ident $(, $arg:expr)* $(,)?) => {
        ::std::io::Error::new(
            ::std::io::ErrorKind::Other,
            ::std::format!("{}: {}", ::std::format_args!($($arg,)+), $error))
    };
}
pub(crate) use io_error;

pub fn json_to_hashmap(value: &serde_json::Value) -> InfoMap<String> {
    let mut hashmap = HashMap::new();

    let object = value.as_object().unwrap();
    for (key, value) in object {
        let string_value = value.as_str().unwrap().to_string();
        hashmap.insert(key.clone(), string_value);
    }

    hashmap
}

pub fn query_to_json(query: &UrlQuery) -> Json<InfoMap<serde_json::Value>> {
    let mut query_as_json = HashMap::new();
    for (k, v) in query.iter() {
        let json_value: serde_json::Value =
            serde_json::from_str(v).unwrap_or_else(|_| serde_json::Value::String(v.clone()));

        query_as_json.insert(k.clone(), json_value);
    }

    Json(query_as_json)
}

pub fn polygon_to_bbox(polygon: &ewkb::Polygon) -> Option<Bounds> {
    polygon.rings().next().and_then(|linestring| {
        let mut points = linestring.points();
        if let (Some(bottom_left), Some(top_right)) = (points.next(), points.nth(1)) {
            Some(Bounds::new(
                bottom_left.x(),
                bottom_left.y(),
                top_right.x(),
                top_right.y(),
            ))
        } else {
            None
        }
    })
}

pub fn parse_x_rewrite_url(header: &HeaderValue) -> Option<String> {
    header
        .to_str()
        .ok()
        .and_then(|header| header.parse::<Uri>().ok())
        .map(|uri| uri.path().to_owned())
}

pub fn create_tilejson(
    name: String,
    minzoom: Option<u8>,
    maxzoom: Option<u8>,
    bounds: Option<Bounds>,
    vector_layers: Option<Vec<VectorLayer>>,
) -> TileJSON {
    let mut tilejson = tilejson! {
        tilejson: "2.2.0".to_string(),
        tiles: vec![],  // tile source is required, but not yet known
        name: name,
    };
    tilejson.minzoom = minzoom;
    tilejson.maxzoom = maxzoom;
    tilejson.bounds = bounds;
    tilejson.vector_layers = vector_layers;

    // TODO: consider removing - this is not needed per TileJSON spec
    tilejson.set_missing_defaults();
    tilejson
}

pub fn is_valid_zoom(zoom: i32, minzoom: Option<u8>, maxzoom: Option<u8>) -> bool {
    minzoom.map_or(true, |minzoom| zoom >= minzoom.into())
        && maxzoom.map_or(true, |maxzoom| zoom <= maxzoom.into())
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::config::{Config, ConfigBuilder};

    pub fn assert_config(yaml: &str, expected: Config) {
        let config: ConfigBuilder = serde_yaml::from_str(yaml).expect("parse yaml");
        let actual = config.finalize().expect("finalize");
        assert_eq!(actual, expected);
    }

    pub fn some_str(s: &str) -> Option<String> {
        Some(s.to_string())
    }
}

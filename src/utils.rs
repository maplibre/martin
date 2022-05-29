use std::collections::HashMap;

use actix_web::http;
use postgis::{ewkb, LineString, Point, Polygon};
use postgres::types::Json;
use serde_json::Value;
use tilejson::Bounds;

use crate::source::{Query, Xyz};

pub fn prettify_error<E: std::fmt::Display>(message: String) -> impl Fn(E) -> std::io::Error {
    move |error| std::io::Error::new(std::io::ErrorKind::Other, format!("{message}: {error}"))
}

// https://github.com/mapbox/postgis-vt-util/blob/master/src/TileBBox.sql
pub fn tilebbox(xyz: &Xyz) -> String {
    let x = xyz.x;
    let y = xyz.y;
    let z = xyz.z;

    let max = 20_037_508.34;
    let res = (max * 2.0) / f64::from(2_i32.pow(z as u32));

    let xmin = -max + (f64::from(x) * res);
    let ymin = max - (f64::from(y) * res);
    let xmax = -max + (f64::from(x) * res) + res;
    let ymax = max - (f64::from(y) * res) - res;

    format!("ST_MakeEnvelope({xmin}, {ymin}, {xmax}, {ymax}, 3857)")
}

pub fn json_to_hashmap(value: &serde_json::Value) -> HashMap<String, String> {
    let mut hashmap = HashMap::new();

    let object = value.as_object().unwrap();
    for (key, value) in object {
        let string_value = value.as_str().unwrap();
        hashmap.insert(key.to_owned(), string_value.to_owned());
    }

    hashmap
}

pub fn query_to_json(query: &Query) -> Json<HashMap<String, Value>> {
    let mut query_as_json = HashMap::new();
    for (k, v) in query.iter() {
        let json_value: serde_json::Value =
            serde_json::from_str(v).unwrap_or_else(|_| serde_json::Value::String(v.clone()));

        query_as_json.insert(k.clone(), json_value);
    }

    Json(query_as_json)
}

pub fn get_bounds_cte(srid_bounds: String) -> String {
    format!(
        include_str!("scripts/get_bounds_cte.sql"),
        srid_bounds = srid_bounds
    )
}

pub fn get_srid_bounds(srid: u32, xyz: &Xyz) -> String {
    format!(
        include_str!("scripts/get_srid_bounds.sql"),
        srid = srid,
        mercator_bounds = tilebbox(xyz),
    )
}

pub fn get_source_bounds(id: &str, srid: u32, geometry_column: &str) -> String {
    format!(
        include_str!("scripts/get_bounds.sql"),
        id = id,
        srid = srid,
        geometry_column = geometry_column,
    )
}

pub fn polygon_to_bbox(polygon: ewkb::Polygon) -> Option<Bounds> {
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

pub fn parse_x_rewrite_url(header: &http::HeaderValue) -> Option<String> {
    header
        .to_str()
        .ok()
        .and_then(|header| header.parse::<http::Uri>().ok())
        .map(|uri| uri.path().trim_end_matches(".json").to_owned())
}

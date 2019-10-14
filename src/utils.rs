use serde_json;
use std::collections::HashMap;

use super::source::{Query, XYZ};

pub fn prettify_error<E: std::fmt::Display>(message: &'static str) -> impl Fn(E) -> std::io::Error {
  move |error| std::io::Error::new(std::io::ErrorKind::Other, format!("{}: {}", message, error))
}

// https://github.com/mapbox/postgis-vt-util/blob/master/src/TileBBox.sql
pub fn tilebbox(xyz: &XYZ) -> String {
  let x = xyz.x;
  let y = xyz.y;
  let z = xyz.z;

  let max = 20_037_508.34;
  let res = (max * 2.0) / f64::from(2_i32.pow(z));

  let xmin = -max + (f64::from(x) * res);
  let ymin = max - (f64::from(y) * res);
  let xmax = -max + (f64::from(x) * res) + res;
  let ymax = max - (f64::from(y) * res) - res;

  format!(
    "ST_MakeEnvelope({0}, {1}, {2}, {3}, 3857)",
    xmin, ymin, xmax, ymax
  )
}

pub fn json_to_hashmap(value: &serde_json::Value) -> HashMap<String, String> {
  let mut hashmap = HashMap::new();

  let object = value.as_object().unwrap();
  for (key, value) in object {
    let string_value = value.as_str().unwrap();
    hashmap.insert(key.to_string(), string_value.to_string());
  }

  hashmap
}

pub fn query_to_json_string(query: &Query) -> Result<String, serde_json::Error> {
  let mut query_as_json = HashMap::new();
  for (k, v) in query.iter() {
    let json_value: serde_json::Value =
      serde_json::from_str(v).unwrap_or_else(|_| serde_json::Value::String(v.clone()));

    query_as_json.insert(k, json_value);
  }

  serde_json::to_string(&query_as_json)
}

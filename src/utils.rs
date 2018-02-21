use serde_json;
use std::collections::HashMap;

// https://github.com/mapbox/postgis-vt-util/blob/master/src/TileBBox.sql
pub fn tilebbox(z: u32, x: u32, y: u32) -> String {
  let max = 20037508.34;
  let res = (max * 2.0) / (2_i32.pow(z) as f64);

  let xmin = -max + (x as f64 * res);
  let ymin = max - (y as f64 * res);
  let xmax = -max + (x as f64 * res) + res;
  let ymax = max - (y as f64 * res) - res;

  format!(
    "ST_MakeEnvelope({0}, {1}, {2}, {3}, 3857)",
    xmin, ymin, xmax, ymax
  )
}

pub fn json_to_hashmap(value: serde_json::Value) -> HashMap<String, String> {
  let mut hashmap = HashMap::new();

  let object = value.as_object().unwrap();
  for (key, value) in object {
    let string_value = value.as_str().unwrap();
    hashmap.insert(key.to_string(), string_value.to_string());
  }

  hashmap
}

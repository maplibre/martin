use actix_web::dev::Params;
use serde_json;
use std::collections::HashMap;

use super::source::XYZ;

pub fn parse_xyz(params: &Params) -> Result<XYZ, &str> {
  let z = params
    .get("z")
    .and_then(|i| i.parse::<u32>().ok())
    .ok_or("invalid z value")?;

  let x = params
    .get("x")
    .and_then(|i| i.parse::<u32>().ok())
    .ok_or("invalid x value")?;

  let y = params
    .get("y")
    .and_then(|i| i.parse::<u32>().ok())
    .ok_or("invalid y value")?;

  Ok(XYZ { x, y, z })
}

// https://github.com/mapbox/postgis-vt-util/blob/master/src/TileBBox.sql
pub fn tilebbox(xyz: XYZ) -> String {
  let x = xyz.x;
  let y = xyz.y;
  let z = xyz.z;

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

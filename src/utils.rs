use actix_web::dev::{ConnectionInfo, Params};
use actix_web::http::header::{HeaderMap, ToStrError};
use serde_json;
use std::collections::HashMap;
use tilejson::{TileJSON, TileJSONBuilder};

use super::app::Query;
use super::source::{Source, XYZ};

pub fn build_tilejson(
  source: Box<dyn Source>,
  connection_info: &ConnectionInfo,
  path: &str,
  query_string: &str,
  headers: &HeaderMap,
) -> Result<TileJSON, ToStrError> {
  let source_id = source.get_id();

  let path =
    headers
      .get("x-rewrite-url")
      .map_or(Ok(path.trim_right_matches(".json")), |header| {
        let header_str = header.to_str()?;
        Ok(header_str.trim_right_matches(".json"))
      })?;

  let query = if query_string.is_empty() {
    query_string.to_owned()
  } else {
    format!("?{}", query_string)
  };

  let tiles_url = format!(
    "{}://{}{}/{{z}}/{{x}}/{{y}}.pbf{}",
    connection_info.scheme(),
    connection_info.host(),
    path,
    query
  );

  let mut tilejson_builder = TileJSONBuilder::new();
  tilejson_builder.scheme("tms");
  tilejson_builder.name(source_id);
  tilejson_builder.tiles(vec![&tiles_url]);

  Ok(tilejson_builder.finalize())
}

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

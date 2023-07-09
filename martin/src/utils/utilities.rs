use std::collections::{BTreeMap, HashMap};
use std::io::{Read as _, Write as _};

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize, Serializer};

#[must_use]
pub fn is_valid_zoom(zoom: u8, minzoom: Option<u8>, maxzoom: Option<u8>) -> bool {
    minzoom.map_or(true, |minzoom| zoom >= minzoom)
        && maxzoom.map_or(true, |maxzoom| zoom <= maxzoom)
}

/// A serde helper to store a boolean as an object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BoolOrObject<T> {
    Bool(bool),
    Object(T),
}

/// Sort an optional hashmap by key, case-insensitive first, then case-sensitive
pub fn sorted_opt_map<S: Serializer, T: Serialize>(
    value: &Option<HashMap<String, T>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    value.as_ref().map(sorted_btree_map).serialize(serializer)
}

/// Sort a hashmap by key, case-insensitive first, then case-sensitive
pub fn sorted_map<S: Serializer, T: Serialize>(
    value: &HashMap<String, T>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    sorted_btree_map(value).serialize(serializer)
}

pub fn sorted_btree_map<K: Serialize + Ord, V>(value: &HashMap<K, V>) -> BTreeMap<&K, &V> {
    let mut items: Vec<(_, _)> = value.iter().collect();
    items.sort_by(|a, b| a.0.cmp(b.0));
    BTreeMap::from_iter(items)
}

pub fn decode_gzip(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = GzDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

pub fn encode_gzip(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data)?;
    encoder.finish()
}

pub fn decode_brotli(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = brotli::Decompressor::new(data, 4096);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

pub fn encode_brotli(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = brotli::CompressorWriter::new(Vec::new(), 4096, 11, 22);
    encoder.write_all(data)?;
    Ok(encoder.into_inner())
}

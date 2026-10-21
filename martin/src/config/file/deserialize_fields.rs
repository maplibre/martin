use serde::de::MapAccess;

pub use martin_config_macros::DeserializeFields;

/// Reads a struct one field at a time from a map that also holds other keys.
///
/// Derived with `#[derive(DeserializeFields)]`.
pub trait DeserializeFields {
    /// Reads the value of `key` from `map` if `key` names a field, and returns whether it did.
    fn deserialize_field<'de, M: MapAccess<'de>>(&mut self, key: &str, map: &mut M) -> Result<bool, M::Error>;
}

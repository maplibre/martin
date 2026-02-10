use serde::{Deserialize, Serialize};

/// A serde helper to store a boolean as an object.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OptBoolObj<T> {
    /// No value present.
    #[default]
    #[serde(skip)]
    NoValue,
    /// A boolean value.
    Bool(bool),
    /// An object value.
    Object(T),
}

impl<T> OptBoolObj<T> {
    /// Returns `true` if this contains no value.
    pub fn is_none(&self) -> bool {
        matches!(self, Self::NoValue)
    }
}

use super::vector_tile::tile::Value;
use std::hash::Hash;

impl From<serde_json::Value> for TileValue {
    fn from(value: serde_json::Value) -> TileValue {
        match value {
            serde_json::Value::String(s) => TileValue::Str(s),
            serde_json::Value::Number(n) => {
                if n.is_f64() {
                    TileValue::Double(n.as_f64().unwrap())
                } else if n.is_i64() {
                    TileValue::Int(n.as_i64().unwrap())
                } else if n.is_u64() {
                    TileValue::Uint(n.as_u64().unwrap())
                } else {
                    // TODO: check
                    unreachable!()
                }
            }
            serde_json::Value::Bool(b) => TileValue::Bool(b),
            _ => TileValue::Str(value.to_string()),
        }
    }
}

/// A wrapper for the MVT value types.
#[derive(Debug, Clone, PartialEq)]
pub enum TileValue {
    Str(String),
    Float(f32),
    Double(f64),
    Int(i64),
    Uint(u64),
    Sint(i64),
    Bool(bool),
}

impl From<TileValue> for Value {
    fn from(tv: TileValue) -> Self {
        match tv {
            TileValue::Str(s) => Self {
                string_value: Some(s),
                ..Default::default()
            },
            TileValue::Float(f) => Self {
                float_value: Some(f),
                ..Default::default()
            },
            TileValue::Double(d) => Self {
                double_value: Some(d),
                ..Default::default()
            },
            TileValue::Int(i) => Self {
                int_value: Some(i),
                ..Default::default()
            },
            TileValue::Uint(u) => Self {
                uint_value: Some(u),
                ..Default::default()
            },
            TileValue::Sint(i) => Self {
                sint_value: Some(i),
                ..Default::default()
            },
            TileValue::Bool(b) => Self {
                bool_value: Some(b),
                ..Default::default()
            },
        }
    }
}

impl TryFrom<Value> for TileValue {
    type Error = ();

    fn try_from(v: Value) -> Result<Self, Self::Error> {
        Ok(if let Some(s) = v.string_value {
            Self::Str(s)
        } else if let Some(f) = v.float_value {
            Self::Float(f)
        } else if let Some(d) = v.double_value {
            Self::Double(d)
        } else if let Some(i) = v.int_value {
            Self::Int(i)
        } else if let Some(u) = v.uint_value {
            Self::Uint(u)
        } else if let Some(i) = v.sint_value {
            Self::Sint(i)
        } else if let Some(b) = v.bool_value {
            Self::Bool(b)
        } else {
            Err(())?
        })
    }
}

// Treat floats as bits so that we can use as keys.
// It is up to the users to ensure that the bits are not NaNs, or are consistent.

impl Eq for TileValue {}

impl Hash for TileValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Self::Str(s) => s.hash(state),
            Self::Float(f) => f.to_bits().hash(state),
            Self::Double(d) => d.to_bits().hash(state),
            Self::Int(i) => i.hash(state),
            Self::Uint(u) => u.hash(state),
            Self::Sint(i) => i.hash(state),
            Self::Bool(b) => b.hash(state),
        }
    }
}

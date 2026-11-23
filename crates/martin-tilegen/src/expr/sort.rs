use std::sync::Arc;

use cel::{ExecutionError, Value};

use super::coerce::mismatch;

#[repr(u8)]
enum SortTag {
    Null,
    Bool,
    Negative,
    NonNegative,
    Double,
    String,
}

/// A `sort_by` result: a scalar or `null`.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum SortValue {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    #[expect(clippy::rc_buffer, reason = "cel's strings are Arc<String>")]
    String(Arc<String>),
}

pub(super) fn to_sort(value: Value) -> Result<SortValue, ExecutionError> {
    Ok(match value {
        Value::Null => SortValue::Null,
        Value::Bool(v) => SortValue::Bool(v),
        Value::Int(v) => SortValue::Int(v),
        Value::UInt(v) => SortValue::UInt(v),
        Value::Float(v) => SortValue::Float(v),
        Value::String(v) => SortValue::String(v),
        other @ (Value::List(_)
        | Value::Map(_)
        | Value::Function(..)
        | Value::Bytes(_)
        | Value::Opaque(_)
        | Value::Struct(_)) => return Err(mismatch(&other, "a scalar")),
    })
}

impl SortValue {
    /// Appends a key whose byte order is this value's order: by type (null, bool, int, double,
    /// string), then by value. `descending` reverses it.
    pub(crate) fn encode(&self, descending: bool, out: &mut Vec<u8>) {
        let start = out.len();
        match self {
            Self::Null => out.push(SortTag::Null as u8),
            Self::Bool(v) => out.extend([SortTag::Bool as u8, u8::from(*v)]),
            Self::Int(v) if *v < 0 => {
                out.push(SortTag::Negative as u8);
                out.extend(v.cast_unsigned().to_be_bytes());
            }
            Self::Int(v) => {
                out.push(SortTag::NonNegative as u8);
                out.extend(v.to_be_bytes());
            }
            Self::UInt(v) => {
                out.push(SortTag::NonNegative as u8);
                out.extend(v.to_be_bytes());
            }
            Self::Float(v) => {
                let bits = v.to_bits();
                let bits = if bits >> 63 == 1 { !bits } else { bits | (1 << 63) };
                out.push(SortTag::Double as u8);
                out.extend(bits.to_be_bytes());
            }
            Self::String(v) => {
                out.push(SortTag::String as u8);
                for &b in v.as_bytes() {
                    if b == 0 {
                        out.extend([0, 0xFF]);
                    } else {
                        out.push(b);
                    }
                }
                out.extend([0, 0]);
            }
        }
        if descending {
            for b in &mut out[start..] {
                *b = !*b;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_keys_order_like_their_values() {
        let ascending = [
            Value::Null,
            Value::Bool(false),
            Value::Bool(true),
            Value::Int(i64::MIN),
            Value::Int(-256),
            Value::Int(-1),
            Value::Int(0),
            Value::UInt(1),
            Value::Int(255),
            Value::UInt(256),
            Value::Int(i64::MAX),
            Value::UInt(u64::MAX),
            Value::Float(f64::NEG_INFINITY),
            Value::Float(-1e10),
            Value::Float(-1.5),
            Value::Float(-0.0),
            Value::Float(0.0),
            Value::Float(1e-300),
            Value::Float(2.5),
            Value::Float(f64::INFINITY),
            Value::from(""),
            Value::from("a"),
            Value::from("a\0"),
            Value::from("a\0b"),
            Value::from("a\x01"),
            Value::from("ab"),
            Value::from("b"),
            Value::from("é"),
        ];
        let keys: Vec<Vec<u8>> = ascending
            .iter()
            .map(|v| {
                let mut key = Vec::new();
                to_sort(v.clone()).unwrap().encode(false, &mut key);
                key
            })
            .collect();
        for (pair, values) in keys.windows(2).zip(ascending.windows(2)) {
            assert!(pair[0] < pair[1], "{:?} < {:?}", values[0], values[1]);
        }
        let descending: Vec<Vec<u8>> = ascending
            .iter()
            .map(|v| {
                let mut key = vec![9];
                to_sort(v.clone()).unwrap().encode(true, &mut key);
                key
            })
            .collect();
        for (pair, values) in descending.windows(2).zip(ascending.windows(2)) {
            assert!(pair[0] > pair[1], "{:?} > {:?} descending", values[0], values[1]);
        }
        assert!(descending.iter().all(|key| key[0] == 9));
    }

    #[test]
    fn a_list_has_no_sort_value() {
        assert_eq!(
            to_sort(Value::from(vec![1])),
            Err(ExecutionError::UnexpectedType {
                got: "list".to_owned(),
                want: "a scalar".to_owned()
            })
        );
    }
}

use std::borrow::Cow;
use std::ops::RangeInclusive;
use std::sync::Arc;

use cel::objects::ValueType;
use cel::{ExecutionError, Value};
use martin_core::generate::props::Prop;

pub(super) fn mismatch(value: &Value, want: &str) -> ExecutionError {
    ExecutionError::UnexpectedType {
        got: value.type_of().to_string(),
        want: want.to_owned(),
    }
}

/// A filter result: `null` is `false`.
pub fn to_filter(value: &Value) -> Result<bool, ExecutionError> {
    match value {
        Value::Null => Ok(false),
        Value::Bool(v) => Ok(*v),
        other @ (Value::List(_)
        | Value::Map(_)
        | Value::Function(..)
        | Value::Int(_)
        | Value::UInt(_)
        | Value::Float(_)
        | Value::String(_)
        | Value::Bytes(_)
        | Value::Opaque(_)
        | Value::Struct(_)) => Err(other.error_expected_type(ValueType::Bool)),
    }
}

/// A zoom level, clamped into `range`.
pub fn to_zoom(value: &Value, range: RangeInclusive<u8>) -> Result<Option<u8>, ExecutionError> {
    let (lo, hi) = (*range.start(), *range.end());
    match *value {
        Value::Null => Ok(None),
        Value::Int(v) => Ok(Some(
            u8::try_from(v.clamp(i64::from(lo), i64::from(hi))).unwrap_or(hi),
        )),
        Value::UInt(v) => Ok(Some(
            u8::try_from(v.clamp(u64::from(lo), u64::from(hi))).unwrap_or(hi),
        )),
        ref other @ (Value::List(_)
        | Value::Map(_)
        | Value::Function(..)
        | Value::Float(_)
        | Value::String(_)
        | Value::Bytes(_)
        | Value::Bool(_)
        | Value::Opaque(_)
        | Value::Struct(_)) => Err(other.error_expected_type(ValueType::Int)),
    }
}

/// A property value; `null` leaves the property out.
pub fn to_prop(value: Value) -> Result<Option<Prop<Cow<'static, str>>>, ExecutionError> {
    Ok(Some(match value {
        Value::Null => return Ok(None),
        Value::Bool(v) => Prop::Bool(v),
        Value::Int(v) => Prop::I64(v),
        Value::UInt(v) => {
            i64::try_from(v).map_or_else(|_| Prop::Str(v.to_string().into()), Prop::I64)
        }
        Value::Float(v) => Prop::F64(v),
        Value::String(v) => Prop::Str(Arc::unwrap_or_clone(v).into()),
        other @ (Value::List(_)
        | Value::Map(_)
        | Value::Function(..)
        | Value::Bytes(_)
        | Value::Opaque(_)
        | Value::Struct(_)) => return Err(mismatch(&other, "a scalar")),
    }))
}

/// A feature id: only a non-negative integer is one.
#[must_use]
pub fn to_id(value: &Value) -> Option<u64> {
    match *value {
        Value::Int(v) => u64::try_from(v).ok(),
        Value::UInt(v) => Some(v),
        Value::Null
        | Value::Bool(_)
        | Value::Float(_)
        | Value::String(_)
        | Value::Bytes(_)
        | Value::List(_)
        | Value::Map(_)
        | Value::Function(..)
        | Value::Opaque(_)
        | Value::Struct(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use martin_core::generate::props::KeyId;
    use rstest::rstest;

    use super::*;
    use crate::expr::{CompiledExpr, ExprKeys, PropSlots};

    #[rstest]
    #[case::null(Value::Null, Ok(false))]
    #[case::t(Value::Bool(true), Ok(true))]
    #[case::f(Value::Bool(false), Ok(false))]
    #[case::int(Value::Int(1), Err(ExecutionError::UnexpectedType { got: "int".to_owned(), want: "bool".to_owned() }))]
    #[case::string(Value::from("yes"), Err(ExecutionError::UnexpectedType { got: "string".to_owned(), want: "bool".to_owned() }))]
    fn filter_coercion(#[case] value: Value, #[case] expected: Result<bool, ExecutionError>) {
        assert_eq!(to_filter(&value), expected);
    }

    #[rstest]
    #[case::null(Value::Null, Ok(None))]
    #[case::inside(Value::Int(7), Ok(Some(7)))]
    #[case::below(Value::Int(-3), Ok(Some(2)))]
    #[case::above(Value::Int(99), Ok(Some(14)))]
    #[case::uint(Value::UInt(u64::MAX), Ok(Some(14)))]
    #[case::double(Value::Float(5.0), Err(ExecutionError::UnexpectedType { got: "float".to_owned(), want: "int".to_owned() }))]
    fn zoom_coercion(#[case] value: Value, #[case] expected: Result<Option<u8>, ExecutionError>) {
        assert_eq!(to_zoom(&value, 2..=14), expected);
    }

    #[rstest]
    #[case::null(Value::Null, Ok(None))]
    #[case::string(Value::from("a"), Ok(Some(Prop::Str("a".into()))))]
    #[case::int(Value::Int(-4), Ok(Some(Prop::I64(-4))))]
    #[case::small_uint(Value::UInt(4), Ok(Some(Prop::I64(4))))]
    #[case::big_uint(Value::UInt(u64::MAX), Ok(Some(Prop::Str("18446744073709551615".into()))))]
    #[case::double(Value::Float(1.5), Ok(Some(Prop::F64(1.5))))]
    #[case::bool(Value::Bool(true), Ok(Some(Prop::Bool(true))))]
    #[case::list(Value::from(vec![1]), Err(ExecutionError::UnexpectedType { got: "list".to_owned(), want: "a scalar".to_owned() }))]
    #[case::map(Value::from(std::collections::HashMap::from([("k", 1)])), Err(ExecutionError::UnexpectedType { got: "map".to_owned(), want: "a scalar".to_owned() }))]
    fn prop_coercion(
        #[case] value: Value,
        #[case] expected: Result<Option<Prop<Cow<'static, str>>>, ExecutionError>,
    ) {
        assert_eq!(to_prop(value), expected);
    }

    #[test]
    fn a_list_result_cannot_be_a_prop() {
        let expr = CompiledExpr::compile("[name]", &["name"], false).unwrap();
        let keys = ExprKeys::columns(&["name"]);
        let props: Vec<(KeyId, Prop)> = vec![];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(
            to_prop(expr.eval(&view).unwrap()),
            Err(ExecutionError::UnexpectedType {
                got: "list".to_owned(),
                want: "a scalar".to_owned()
            })
        );
    }

    #[rstest]
    #[case::int(Value::Int(42), Some(42))]
    #[case::zero(Value::Int(0), Some(0))]
    #[case::negative(Value::Int(-1), None)]
    #[case::uint(Value::UInt(u64::MAX), Some(u64::MAX))]
    #[case::double(Value::Float(3.0), None)]
    #[case::string(Value::from("42"), None)]
    #[case::null(Value::Null, None)]
    fn id_coercion(#[case] value: Value, #[case] expected: Option<u64>) {
        assert_eq!(to_id(&value), expected);
    }
}

//! A table's [`Condition`] checked on OSM objects: whether an object type can match at all, whether a block's strings
//! can hold a match, and whether one object matches, each with SQL's three-valued logic.

use std::cmp::Ordering;

use osmflow::BlockStrings;

use crate::plan::{CompareOp, Condition, GeometryType, Literal, Pattern, Polarity, Term, ValueKind};

/// The values a [`Condition`] reads from one object: its tags by column name, and its geometry type.
pub(crate) trait Row {
    /// The value of column `name`, `None` if the object has no such tag.
    fn column(&self, name: &str) -> Option<&str>;

    /// The type of the object's geometry.
    fn geometry(&self) -> GeometryType;
}

/// Whether `condition` holds, each comparison, null check, list and pattern decided by `leaf`.
/// A [`Condition`] carries its negations in its leaves, so a `leaf` that says `true` when unsure gives a superset.
fn holds(condition: &Condition, leaf: &mut impl FnMut(&Condition) -> bool) -> bool {
    match condition {
        Condition::Const(value) => *value,
        Condition::And(parts) => parts.iter().all(|part| holds(part, leaf)),
        Condition::Or(parts) => parts.iter().any(|part| holds(part, leaf)),
        Condition::Compare { .. }
        | Condition::Distinct { .. }
        | Condition::IsNull { .. }
        | Condition::In { .. }
        | Condition::Like { .. }
        | Condition::Geometry(_) => leaf(condition),
    }
}

/// Whether some object whose geometry is one of `types` can match `condition`, whatever its tags.
pub(crate) fn may_have_type(condition: &Condition, types: &[GeometryType]) -> bool {
    holds(condition, &mut |leaf| match leaf {
        Condition::Geometry(kind) => types.contains(kind),
        Condition::Const(_)
        | Condition::And(_)
        | Condition::Or(_)
        | Condition::Compare { .. }
        | Condition::Distinct { .. }
        | Condition::IsNull { .. }
        | Condition::In { .. }
        | Condition::Like { .. } => true,
    })
}

/// Whether a block with `strings` may hold an object whose geometry is one of `types` that matches `condition`.
/// `is_tag` tells which columns are tags: only their keys and values are among a block's strings.
pub(crate) fn may_be_in_block(
    condition: &Condition,
    types: &[GeometryType],
    is_tag: &impl Fn(&str) -> bool,
    strings: &BlockStrings<'_>,
) -> bool {
    holds(condition, &mut |leaf| {
        let key_used = |term: &Term| {
            tag_column(term)
                .filter(|name| is_tag(name))
                .is_none_or(|key| strings.has(key))
        };
        match leaf {
            Condition::Geometry(kind) => types.contains(kind),
            Condition::Compare { left, op, right, .. } => {
                let value_used = match (op, left, right) {
                    (CompareOp::Eq, Term::Column(name), Term::Literal(Literal::String(value)))
                    | (CompareOp::Eq, Term::Literal(Literal::String(value)), Term::Column(name))
                        if is_tag(name) =>
                    {
                        strings.has(value)
                    }
                    _ => true,
                };
                key_used(left) && key_used(right) && value_used
            }
            Condition::IsNull {
                term,
                polarity: Polarity::Negated,
            } => key_used(term),
            Condition::In {
                term, values, polarity, ..
            } => {
                let value_used = match (polarity, term) {
                    (Polarity::AsWritten, Term::Column(name)) if is_tag(name) => values
                        .iter()
                        .any(|value| !matches!(value, Literal::String(s) if !strings.has(s))),
                    _ => true,
                };
                key_used(term) && value_used
            }
            Condition::Like {
                term,
                pattern,
                polarity,
            } => {
                let value_used = match (polarity, term, pattern) {
                    (Polarity::AsWritten, Term::Column(name), Pattern::Prefix(prefix)) if is_tag(name) => {
                        strings.has_prefix(prefix)
                    }
                    _ => true,
                };
                key_used(term) && value_used
            }
            Condition::IsNull {
                polarity: Polarity::AsWritten,
                ..
            }
            | Condition::Distinct { .. }
            | Condition::Const(_)
            | Condition::And(_)
            | Condition::Or(_) => true,
        }
    })
}

/// The column a term reads, if it reads one.
fn tag_column(term: &Term) -> Option<&str> {
    match term {
        Term::Column(name) => Some(name),
        Term::Length(inner) | Term::LowerAscii(inner) | Term::UpperAscii(inner) => tag_column(inner),
        Term::Literal(_) => None,
    }
}

/// Whether `row` matches `condition`; `true` also where it cannot tell, as for a value of an unexpected kind.
pub(crate) fn matches(condition: &Condition, row: &impl Row) -> bool {
    holds(condition, &mut |leaf| leaf_matches(leaf, row).unwrap_or(true))
}

/// A leaf or value that cannot be decided here, as for a value of an unexpected kind.
struct Unknown;

/// A value of a row or a literal.
#[derive(Debug, PartialEq)]
enum Value<'a> {
    /// A boolean literal.
    Bool(bool),
    /// An integer literal, or the length of a string.
    Int(i64),
    /// A float literal.
    Float(f64),
    /// A tag value, a string literal, or a string made from one.
    Str(std::borrow::Cow<'a, str>),
}

/// The leaf's truth for `row`, a null counting as false.
fn leaf_matches(leaf: &Condition, row: &impl Row) -> Result<bool, Unknown> {
    Ok(match leaf {
        Condition::Geometry(kind) => row.geometry() == *kind,
        Condition::Compare { left, op, right, kind } => {
            let (Some(left), Some(right)) = (value(left, row)?, value(right, row)?) else {
                return Ok(false);
            };
            let order = compare(&left, &right, *kind)?;
            match op {
                CompareOp::Eq => order == Ordering::Equal,
                CompareOp::Ne => order != Ordering::Equal,
                CompareOp::Lt => order == Ordering::Less,
                CompareOp::Le => order != Ordering::Greater,
                CompareOp::Gt => order == Ordering::Greater,
                CompareOp::Ge => order != Ordering::Less,
            }
        }
        Condition::Distinct {
            left,
            right,
            kind,
            polarity,
        } => {
            let distinct = match (value(left, row)?, value(right, row)?) {
                (None, None) => false,
                (Some(left), Some(right)) => compare(&left, &right, *kind)? != Ordering::Equal,
                _ => true,
            };
            distinct != polarity.negated()
        }
        Condition::IsNull { term, polarity } => value(term, row)?.is_none() != polarity.negated(),
        Condition::In {
            term,
            values,
            kind,
            polarity,
        } => {
            let Some(value) = value(term, row)? else {
                return Ok(false);
            };
            let mut found = false;
            for literal in values {
                found |= compare(&value, &literal_value(literal), *kind)? == Ordering::Equal;
            }
            found != polarity.negated()
        }
        Condition::Like {
            term,
            pattern,
            polarity,
        } => {
            let Some(Value::Str(text)) = value(term, row)? else {
                return Ok(false);
            };
            let found = match pattern {
                Pattern::Prefix(p) => text.starts_with(p.as_str()),
                Pattern::Suffix(p) => text.ends_with(p.as_str()),
                Pattern::Contains(p) => text.contains(p.as_str()),
            };
            found != polarity.negated()
        }
        Condition::Const(_) | Condition::And(_) | Condition::Or(_) => return Err(Unknown),
    })
}

/// The value of `term` in `row`, `None` for null.
fn value<'a>(term: &'a Term, row: &'a impl Row) -> Result<Option<Value<'a>>, Unknown> {
    Ok(match term {
        Term::Column(name) => row.column(name).map(|text| Value::Str(text.into())),
        Term::Literal(literal) => Some(literal_value(literal)),
        Term::Length(inner) => string_function(inner, row, |text| {
            Ok(Value::Int(
                i64::try_from(text.chars().count()).map_err(|_too_long| Unknown)?,
            ))
        })?,
        Term::LowerAscii(inner) => {
            string_function(inner, row, |text| Ok(Value::Str(text.to_ascii_lowercase().into())))?
        }
        Term::UpperAscii(inner) => {
            string_function(inner, row, |text| Ok(Value::Str(text.to_ascii_uppercase().into())))?
        }
    })
}

/// The value of `f` applied to the text of `inner` in `row`, `None` for null; unknown if `inner` is not text.
fn string_function<'a>(
    inner: &'a Term,
    row: &'a impl Row,
    f: impl FnOnce(&str) -> Result<Value<'a>, Unknown>,
) -> Result<Option<Value<'a>>, Unknown> {
    match value(inner, row)? {
        Some(Value::Str(text)) => f(&text).map(Some),
        None => Ok(None),
        Some(_) => Err(Unknown),
    }
}

/// The value of a literal; an unsigned integer too large for `i64` is NaN, which compares as unknown.
fn literal_value(literal: &Literal) -> Value<'_> {
    match literal {
        Literal::Bool(value) => Value::Bool(*value),
        Literal::Int(value) => Value::Int(*value),
        Literal::UInt(value) => i64::try_from(*value).map_or(Value::Float(f64::NAN), Value::Int),
        Literal::Float(value) => Value::Float(*value),
        Literal::String(value) => Value::Str(value.as_str().into()),
    }
}

/// How `left` orders against `right`, both of `kind`; unknown if either is of another kind, or a float is NaN.
fn compare(left: &Value<'_>, right: &Value<'_>, kind: ValueKind) -> Result<Ordering, Unknown> {
    match (kind, left, right) {
        (ValueKind::String, Value::Str(left), Value::Str(right)) => Ok(left.cmp(right)),
        (ValueKind::Int, Value::Int(left), Value::Int(right)) => Ok(left.cmp(right)),
        (ValueKind::Bool, Value::Bool(left), Value::Bool(right)) => Ok(left.cmp(right)),
        (ValueKind::Float, _, _) => float(left)?.partial_cmp(&float(right)?).ok_or(Unknown),
        _ => Err(Unknown),
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "SQL compares an integer with a float as floats"
)]
/// A number as a float, as SQL compares an integer with a float.
fn float(value: &Value<'_>) -> Result<f64, Unknown> {
    match value {
        Value::Int(value) => Ok(*value as f64),
        Value::Float(value) => Ok(*value),
        Value::Bool(_) | Value::Str(_) => Err(Unknown),
    }
}

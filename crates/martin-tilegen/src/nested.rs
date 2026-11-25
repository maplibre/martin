//! Nested columns from JSON documents: the shape a column's documents share, and a document as a
//! value of it.

use std::collections::BTreeMap;

use mlt_core::{NestedKind, NestedValue, PropKind, PropValue};
use serde_json::Value;

/// A document read from its JSON text; text that is not JSON is a string.
#[must_use]
pub fn parse(json: &str) -> Value {
    serde_json::from_str(json).unwrap_or_else(|_| Value::String(json.to_owned()))
}

/// How many levels a nested column may have, counting its root as the first.
const MAX_DEPTH: usize = 8;

/// The union of the shapes of the documents a column holds.
///
/// Values that share no shape are kept as their JSON text.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Shape {
    /// Nothing but `null` so far.
    #[default]
    Null,
    /// A scalar of this kind.
    Leaf(PropKind),
    /// A list whose elements share one shape.
    List(Box<Self>),
    /// A map from each key seen to the shape of its values.
    Map(BTreeMap<String, Self>),
}

impl Shape {
    /// The shape of one `document`.
    #[must_use]
    pub fn of(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(_) => Self::Leaf(PropKind::Bool),
            Value::Number(n) if n.is_i64() => Self::Leaf(PropKind::I64),
            Value::Number(_) => Self::Leaf(PropKind::F64),
            Value::String(_) => Self::Leaf(PropKind::Str),
            Value::Array(items) => Self::List(Box::new(items.iter().map(Self::of).fold(Self::Null, Self::merge))),
            Value::Object(entries) => Self::Map(
                entries
                    .iter()
                    .map(|(key, value)| (key.clone(), Self::of(value)))
                    .collect(),
            ),
        }
    }

    /// The shape both `self` and `other` fit in.
    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Null, shape) | (shape, Self::Null) => shape,
            (Self::Leaf(a), Self::Leaf(b)) => Self::Leaf(match (a, b) {
                _ if a == b => a,
                (PropKind::I64, PropKind::F64) | (PropKind::F64, PropKind::I64) => PropKind::F64,
                _ => PropKind::Str,
            }),
            (Self::List(a), Self::List(b)) => Self::List(Box::new(a.merge(*b))),
            (Self::Map(mut a), Self::Map(b)) => {
                for (key, shape) in b {
                    let merged = a.remove(&key).unwrap_or_default().merge(shape);
                    a.insert(key, merged);
                }
                Self::Map(a)
            }
            _ => Self::Leaf(PropKind::Str),
        }
    }

    /// The column shape for this one, or `None` when it holds nothing a tile can keep.
    ///
    /// A field that only ever held `null` or `{}` is left out, and a node too deep to have
    /// children is kept as JSON text.
    #[must_use]
    pub fn kind(self) -> Option<NestedKind> {
        self.kind_at(1)
    }

    fn kind_at(self, depth: usize) -> Option<NestedKind> {
        match self {
            Self::Null => None,
            Self::Leaf(kind) => Some(NestedKind::Leaf(kind)),
            _ if depth >= MAX_DEPTH => Some(NestedKind::Leaf(PropKind::Str)),
            Self::List(element) => Some(NestedKind::list(
                element.kind_at(depth + 1).unwrap_or(NestedKind::Leaf(PropKind::Str)),
            )),
            Self::Map(fields) => {
                let fields: BTreeMap<_, _> = fields
                    .into_iter()
                    .filter_map(|(key, shape)| Some((key, shape.kind_at(depth + 1)?)))
                    .collect();
                (!fields.is_empty()).then_some(NestedKind::Map(fields))
            }
        }
    }
}

/// A document as a value of the column shape `kind`.
#[must_use]
pub fn nested_value(document: Value, kind: &NestedKind) -> NestedValue {
    match (kind, document) {
        (_, Value::Null) => kind.null_value(),
        (NestedKind::Leaf(kind), document) => NestedValue::Leaf(leaf_value(*kind, document)),
        (NestedKind::List(element), Value::Array(items)) => NestedValue::List(Some(
            items.into_iter().map(|item| nested_value(item, element)).collect(),
        )),
        (NestedKind::Map(fields), Value::Object(entries)) => NestedValue::Map(Some(
            entries
                .into_iter()
                .filter(|(_, value)| !value.is_null())
                .filter_map(|(key, entry)| {
                    let kind = fields.get(&key)?;
                    Some((key.into(), nested_value(entry, kind)))
                })
                .collect(),
        )),
        (NestedKind::List(_) | NestedKind::Map(_), _) => kind.null_value(),
    }
}

/// A scalar as a value of `kind`, anything that is not one of those as its JSON text.
#[must_use]
pub fn leaf_value(kind: PropKind, document: Value) -> PropValue {
    match (kind, document) {
        (PropKind::Bool, Value::Bool(b)) => PropValue::Bool(Some(b)),
        (PropKind::I64, Value::Number(n)) => PropValue::I64(n.as_i64()),
        (PropKind::F64, Value::Number(n)) => PropValue::F64(n.as_f64()),
        (PropKind::Str, Value::String(s)) => PropValue::Str(Some(s.into())),
        (PropKind::Str, document) => PropValue::Str(Some(document.to_string().into())),
        (kind, _) => PropValue::null(kind),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn kind_of(documents: &[Value]) -> Option<NestedKind> {
        documents.iter().map(Shape::of).fold(Shape::Null, Shape::merge).kind()
    }

    #[test]
    fn merges_numbers_and_widens_to_text() {
        let kind = kind_of(&[json!({"a": 1, "b": 1}), json!({"a": 1.5, "b": "x"})]);
        assert_eq!(
            kind,
            Some(NestedKind::Map(BTreeMap::from([
                ("a".to_owned(), NestedKind::Leaf(PropKind::F64)),
                ("b".to_owned(), NestedKind::Leaf(PropKind::Str)),
            ])))
        );
    }

    #[test]
    fn leaves_out_nulls_and_empty_maps() {
        assert_eq!(kind_of(&[json!({"a": null, "b": {}})]), None);
        assert_eq!(kind_of(&[json!(null)]), None);
    }

    #[test]
    fn list_of_nothing_is_text() {
        assert_eq!(
            kind_of(&[json!({"links": []})]),
            Some(NestedKind::Map(BTreeMap::from([(
                "links".to_owned(),
                NestedKind::list(NestedKind::Leaf(PropKind::Str))
            )])))
        );
    }

    #[test]
    fn too_deep_is_kept_as_text() {
        let mut document = json!(1);
        for _ in 0..MAX_DEPTH + 2 {
            document = json!({ "k": document });
        }
        let mut kind = kind_of(&[document]).expect("not empty");
        let mut depth = 1;
        while let NestedKind::Map(mut fields) = kind {
            kind = fields.remove("k").expect("one key");
            depth += 1;
        }
        assert_eq!(kind, NestedKind::Leaf(PropKind::Str));
        assert_eq!(depth, MAX_DEPTH);
    }

    #[test]
    fn value_follows_the_shape() {
        let documents = [
            json!({"tags": ["a", "b"], "n": 2, "o": {"x": true}}),
            json!({"n": null}),
        ];
        let kind = kind_of(&documents).expect("not empty");
        let [first, second] = documents;
        assert_eq!(
            nested_value(first, &kind),
            NestedValue::Map(Some(BTreeMap::from([
                ("n".into(), NestedValue::Leaf(PropValue::I64(Some(2)))),
                (
                    "o".into(),
                    NestedValue::Map(Some(BTreeMap::from([(
                        "x".into(),
                        NestedValue::Leaf(PropValue::Bool(Some(true)))
                    )])))
                ),
                (
                    "tags".into(),
                    NestedValue::List(Some(vec![
                        NestedValue::Leaf(PropValue::Str(Some("a".into()))),
                        NestedValue::Leaf(PropValue::Str(Some("b".into()))),
                    ]))
                ),
            ])))
        );
        assert_eq!(nested_value(second, &kind), NestedValue::Map(Some(BTreeMap::new())));
    }

    #[test]
    fn mismatched_scalars_are_json_text() {
        assert_eq!(
            leaf_value(PropKind::Str, json!({"a": 1})),
            PropValue::Str(Some(r#"{"a":1}"#.into()))
        );
        assert_eq!(leaf_value(PropKind::I64, json!("x")), PropValue::null(PropKind::I64));
    }
}

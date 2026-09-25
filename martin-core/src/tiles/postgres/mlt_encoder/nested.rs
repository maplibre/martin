//! `jsonb` documents kept whole, as the nested columns of the v2 wire format.

use std::collections::{BTreeMap, HashMap};

use mlt_core::{
    MltResult, NestedKey, NestedKind, NestedValue, PropKind, PropValue, PropertyKey,
    TileFeatureBuilder, TileLayerBuilder,
};
use serde_json::Value;

/// How many levels a nested column may have, counting its root as the first.
const MAX_DEPTH: usize = 8;

/// The `jsonb` columns of one tile, each with the shape of every document it holds.
#[derive(Default)]
pub(super) struct Documents {
    names: Vec<String>,
    index: HashMap<String, usize>,
    shapes: Vec<Shape>,
}

impl Documents {
    /// Files one feature's `document` for the column `name` into `docs`.
    pub(super) fn push(
        &mut self,
        name: &str,
        document: Option<Value>,
        docs: &mut Vec<(usize, Value)>,
    ) {
        let Some(document) = document.filter(|d| !d.is_null()) else {
            return;
        };
        let shape = Shape::of(&document);
        let idx = if let Some(&idx) = self.index.get(name) {
            let merged = std::mem::take(&mut self.shapes[idx]).merge(shape);
            self.shapes[idx] = merged;
            idx
        } else {
            let idx = self.names.len();
            self.names.push(name.to_owned());
            self.index.insert(name.to_owned(), idx);
            self.shapes.push(shape);
            idx
        };
        docs.push((idx, document));
    }

    /// Declares a column for every `jsonb` column holding anything a tile can keep.
    ///
    /// A column whose documents are all scalars is an ordinary property column.
    pub(super) fn declare(self, builder: &mut TileLayerBuilder) -> MltResult<Vec<Option<Column>>> {
        self.names
            .into_iter()
            .zip(self.shapes)
            .map(|(name, shape)| {
                Ok(match shape.kind(1) {
                    None => None,
                    Some(NestedKind::Leaf(kind)) => {
                        Some(Column::Scalar(builder.add_property(name, kind)?, kind))
                    }
                    Some(kind) => Some(Column::Nested(
                        builder.add_nested(name, kind.clone())?,
                        kind,
                    )),
                })
            })
            .collect()
    }
}

/// Where a `jsonb` column's documents go in the layer.
pub(super) enum Column {
    Scalar(PropertyKey, PropKind),
    Nested(NestedKey, NestedKind),
}

impl Column {
    pub(super) fn set(
        &self,
        feature: &mut TileFeatureBuilder<'_>,
        document: Value,
    ) -> MltResult<()> {
        match self {
            Self::Scalar(key, kind) => feature.property(*key, leaf(*kind, document))?,
            Self::Nested(key, kind) => feature.nested(*key, value(document, kind))?,
        };
        Ok(())
    }
}

/// The union of the shapes of the documents a column holds.
///
/// Values that share no shape are kept as their JSON text.
#[derive(Debug, Clone, Default, PartialEq)]
enum Shape {
    #[default]
    Null,
    Leaf(PropKind),
    List(Box<Self>),
    Map(BTreeMap<String, Self>),
}

impl Shape {
    fn of(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(_) => Self::Leaf(PropKind::Bool),
            Value::Number(n) if n.is_i64() => Self::Leaf(PropKind::I64),
            Value::Number(_) => Self::Leaf(PropKind::F64),
            Value::String(_) => Self::Leaf(PropKind::Str),
            Value::Array(items) => Self::List(Box::new(
                items.iter().map(Self::of).fold(Self::Null, Self::merge),
            )),
            Value::Object(entries) => Self::Map(
                entries
                    .iter()
                    .map(|(key, value)| (key.clone(), Self::of(value)))
                    .collect(),
            ),
        }
    }

    fn merge(self, other: Self) -> Self {
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

    /// The column shape for this one at `depth`, or `None` when it holds nothing a tile can keep.
    ///
    /// A field that only ever held `null` or `{}` is left out, and a node too deep to have
    /// children is kept as JSON text.
    fn kind(self, depth: usize) -> Option<NestedKind> {
        match self {
            Self::Null => None,
            Self::Leaf(kind) => Some(NestedKind::Leaf(kind)),
            _ if depth >= MAX_DEPTH => Some(NestedKind::Leaf(PropKind::Str)),
            Self::List(element) => Some(NestedKind::list(
                element
                    .kind(depth + 1)
                    .unwrap_or(NestedKind::Leaf(PropKind::Str)),
            )),
            Self::Map(fields) => {
                let fields: BTreeMap<_, _> = fields
                    .into_iter()
                    .filter_map(|(key, shape)| Some((key, shape.kind(depth + 1)?)))
                    .collect();
                (!fields.is_empty()).then_some(NestedKind::Map(fields))
            }
        }
    }
}

/// A document as a value of the column shape `kind`.
fn value(document: Value, kind: &NestedKind) -> NestedValue {
    match (kind, document) {
        (_, Value::Null) => kind.null_value(),
        (NestedKind::Leaf(kind), document) => NestedValue::Leaf(leaf(*kind, document)),
        (NestedKind::List(element), Value::Array(items)) => NestedValue::List(Some(
            items.into_iter().map(|item| value(item, element)).collect(),
        )),
        (NestedKind::Map(fields), Value::Object(entries)) => NestedValue::Map(Some(
            entries
                .into_iter()
                .filter(|(_, value)| !value.is_null())
                .filter_map(|(key, entry)| {
                    let kind = fields.get(&key)?;
                    Some((key, value(entry, kind)))
                })
                .collect(),
        )),
        (NestedKind::List(_) | NestedKind::Map(_), _) => kind.null_value(),
    }
}

/// A scalar as a value of `kind`, anything that is not one of those as its JSON text.
fn leaf(kind: PropKind, document: Value) -> PropValue {
    match (kind, document) {
        (PropKind::Bool, Value::Bool(b)) => PropValue::Bool(Some(b)),
        (PropKind::I64, Value::Number(n)) => PropValue::I64(n.as_i64()),
        (PropKind::F64, Value::Number(n)) => PropValue::F64(n.as_f64()),
        (PropKind::Str, Value::String(s)) => PropValue::Str(Some(s)),
        (PropKind::Str, document) => PropValue::Str(Some(document.to_string())),
        (kind, _) => PropValue::null(kind),
    }
}

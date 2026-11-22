//! `jsonb` documents kept whole, as the nested columns of the v2 wire format.

use std::collections::HashMap;

use martin_tilegen::nested::{Shape, leaf_value, nested_value};
use mlt_core::{MltResult, NestedKey, NestedKind, PropKind, PropertyKey, TileFeatureBuilder, TileLayerBuilder};
use serde_json::Value;

/// The `jsonb` columns of one tile, each with the shape of every document it holds.
#[derive(Default)]
pub(super) struct Documents {
    names: Vec<String>,
    index: HashMap<String, usize>,
    shapes: Vec<Shape>,
}

impl Documents {
    /// Files one feature's `document` for the column `name` into `docs`.
    pub(super) fn push(&mut self, name: &str, document: Option<Value>, docs: &mut Vec<(usize, Value)>) {
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
                Ok(match shape.kind() {
                    None => None,
                    Some(NestedKind::Leaf(kind)) => Some(Column::Scalar(builder.add_property(name, kind)?, kind)),
                    Some(kind) => Some(Column::Nested(builder.add_nested(name, kind.clone())?, kind)),
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
    pub(super) fn set(&self, feature: &mut TileFeatureBuilder<'_>, document: Value) -> MltResult<()> {
        match self {
            Self::Scalar(key, kind) => feature.property(*key, leaf_value(*kind, document))?,
            Self::Nested(key, kind) => feature.nested(*key, nested_value(document, kind))?,
        };
        Ok(())
    }
}

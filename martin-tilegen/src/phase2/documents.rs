//! The `jsonb` documents of one layer in one tile, written as the nested and property columns of the
//! v2 wire format.

use std::slice::Iter;

use mlt_core::{FeatureWriter, LayerWriter, MltResult, NestedKey, NestedKind, PropValue, PropValueRef};

use super::columns::{TileColumns, column_order};
use crate::nested::{leaf_value, nested_value, parse};
use crate::props::{DocumentColumn, KeyId, KeyNames};

/// Reusable per-worker scratch for the document keys of a layer.
#[derive(Default)]
pub(super) struct Documents {
    /// The keys of nested columns that some feature of the tile carries.
    present: Vec<KeyId>,
    /// The nested columns of the layer, declared by [`declare`](Self::declare).
    declared: Vec<(KeyId, NestedKey, NestedKind)>,
    /// The values of the scalar document keys, in the order the tile's features carry them.
    leaves: Vec<PropValue>,
}

impl Documents {
    pub(super) fn clear(&mut self) {
        self.present.clear();
        self.declared.clear();
        self.leaves.clear();
    }

    /// Files `key`'s column if it is a document key, and says whether it was: the key is then not an
    /// ordinary property.
    pub(super) fn note(&mut self, names: &KeyNames, columns: &mut TileColumns, key: KeyId) -> bool {
        match names.document(key) {
            None => return false,
            Some(DocumentColumn::Scalar(kind)) => columns.add_document(key, *kind),
            Some(DocumentColumn::Nested(_)) => {
                if !self.present.contains(&key) {
                    self.present.push(key);
                }
            }
            Some(DocumentColumn::Dropped) => {}
        }
        true
    }

    /// Declares a nested column for every key some feature of the tile carries.
    pub(super) fn declare(&mut self, names: &KeyNames, layer: &mut LayerWriter<'_>) -> MltResult<()> {
        self.present.sort_by(|&a, &b| column_order(names, a, b));
        for &key in &self.present {
            if let Some(DocumentColumn::Nested(kind)) = names.document(key) {
                let column = layer.add_nested(names.name(key), kind.clone())?;
                self.declared.push((key, column, kind.clone()));
            }
        }
        Ok(())
    }

    /// Reads the scalar document `value` of `key` as the column's kind, for [`Leaves`] to hand out.
    pub(super) fn collect_leaf(&mut self, names: &KeyNames, key: KeyId, value: PropValueRef<'_>) {
        if let (Some(DocumentColumn::Scalar(kind)), PropValueRef::Str(json)) = (names.document(key), value) {
            self.leaves.push(leaf_value(*kind, parse(json)));
        }
    }

    /// The columns and leaves to write the tile's features with, once they are all collected.
    pub(super) fn writer(&self) -> DocumentWriter<'_> {
        DocumentWriter {
            declared: &self.declared,
            leaves: self.leaves.iter(),
        }
    }
}

/// Writes the document values of a tile's features, in the order they were collected.
pub(super) struct DocumentWriter<'d> {
    declared: &'d [(KeyId, NestedKey, NestedKind)],
    leaves: Iter<'d, PropValue>,
}

impl<'d> DocumentWriter<'d> {
    /// Writes the document `value` of `key`, a document key, into `feature`; `property` is the key's
    /// property column when it is a scalar one.
    pub(super) fn write<'a>(
        &mut self,
        names: &KeyNames,
        feature: &mut FeatureWriter<'_, 'a>,
        key: KeyId,
        value: PropValueRef<'_>,
        property: Option<mlt_core::PropertyKey>,
    ) -> MltResult<()>
    where
        'd: 'a,
    {
        let PropValueRef::Str(json) = value else {
            return Ok(());
        };
        match names.document(key) {
            Some(DocumentColumn::Scalar(_)) => {
                if let (Some(column), Some(leaf)) = (property, self.leaves.next())
                    && let Some(value) = leaf.value_ref()
                {
                    feature.property(column, value)?;
                }
            }
            Some(DocumentColumn::Nested(_)) => {
                if let Some((_, column, kind)) = self.declared.iter().find(|(k, ..)| *k == key) {
                    feature.nested(*column, nested_value(parse(json), kind))?;
                }
            }
            Some(DocumentColumn::Dropped) | None => {}
        }
        Ok(())
    }
}

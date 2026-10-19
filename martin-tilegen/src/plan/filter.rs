//! Which rows of a table its layers keep, as a condition a source can check before reading them.

use crate::plan::GeometryType;

/// The rows a source has to read for a table.
#[derive(Clone, Debug, PartialEq)]
pub enum RowFilter {
    /// Every row: some layer takes all of them.
    All,
    /// Exactly the rows matching this.
    Matching(Condition),
    /// Every row, as some layer filters on something a [`Condition`] cannot express.
    Untranslatable(Untranslatable),
}

impl RowFilter {
    /// The condition a source filters on, if any.
    #[must_use]
    pub fn condition(&self) -> Option<&Condition> {
        match self {
            Self::Matching(condition) => Some(condition),
            Self::All | Self::Untranslatable(_) => None,
        }
    }
}

/// A layer's `where` that no [`Condition`] expresses.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("layer `{layer}`: `{expr}` {reason}")]
pub struct Untranslatable {
    pub layer: String,
    pub expr: String,
    pub reason: TranslateError,
}

/// Why an expression has no [`Condition`].
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TranslateError {
    #[error("is not checked in SQL yet")]
    Where,
}

/// A condition on a row with SQL's three-valued logic: the row passes only when it is true.
#[derive(Clone, Debug, PartialEq)]
pub enum Condition {
    Const(bool),
    And(Vec<Self>),
    Or(Vec<Self>),
    /// The geometry is of this type, single or multi.
    Geometry(GeometryType),
}

impl Condition {
    /// `parts` all hold, flattened, with constants folded.
    pub(crate) fn and(parts: impl IntoIterator<Item = Self>) -> Self {
        Self::fold(parts, true)
    }

    /// One of `parts` holds, flattened, with constants folded.
    pub(crate) fn or(parts: impl IntoIterator<Item = Self>) -> Self {
        Self::fold(parts, false)
    }

    /// `and` when `all`, else `or`.
    fn fold(parts: impl IntoIterator<Item = Self>, all: bool) -> Self {
        let mut kept = Vec::new();
        for part in parts {
            match (part, all) {
                (Self::Const(value), _) if value == all => {}
                (Self::Const(_), _) => return Self::Const(!all),
                (Self::And(inner), true) | (Self::Or(inner), false) => kept.extend(inner),
                (part, _) => kept.push(part),
            }
        }
        match kept.len() {
            0 => Self::Const(all),
            1 => kept.pop().expect("one part"),
            _ if all => Self::And(kept),
            _ => Self::Or(kept),
        }
    }
}

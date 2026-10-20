//! Templates for the ids of auto-published sources.

use std::fmt::{self, Display, Formatter};
use std::marker::PhantomData;

use itertools::Itertools as _;
use serde::{Deserialize, Serialize};

/// The placeholders one kind of source id may use.
pub trait IdPlaceholders: Clone {
    /// Placeholder names without braces.
    const NAMES: &'static [&'static str];
}

/// The placeholders of a table source id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableIds {}

impl IdPlaceholders for TableIds {
    const NAMES: &'static [&'static str] = &["schema", "table", "column"];
}

/// The placeholders of a function source id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FunctionIds {}

impl IdPlaceholders for FunctionIds {
    const NAMES: &'static [&'static str] = &["schema", "function"];
}

/// A source id template whose every `{name}` is a placeholder of `P`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String", bound = "")]
pub struct IdFormat<P: IdPlaceholders> {
    format: String,
    placeholders: PhantomData<P>,
}

/// The id template of an auto-published table.
pub type TableIdFormat = IdFormat<TableIds>;

/// The id template of an auto-published function.
pub type FunctionIdFormat = IdFormat<FunctionIds>;

/// A `{name}` in an id template that is not a known placeholder.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("source id format {format:?} uses the unknown placeholder {{{name}}}, it may use {known}")]
pub struct UnknownIdPlaceholder {
    format: String,
    name: String,
    known: String,
}

impl<P: IdPlaceholders> IdFormat<P> {
    /// The template as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.format
    }
}

impl<P: IdPlaceholders> TryFrom<String> for IdFormat<P> {
    type Error = UnknownIdPlaceholder;

    fn try_from(format: String) -> Result<Self, Self::Error> {
        let mut rest = format.as_str();
        while let Some((_, after)) = rest.split_once('{') {
            let Some((name, tail)) = after.split_once('}') else {
                break;
            };
            if !P::NAMES.contains(&name) {
                return Err(UnknownIdPlaceholder {
                    name: name.to_owned(),
                    known: P::NAMES.iter().map(|name| format!("{{{name}}}")).join(", "),
                    format,
                });
            }
            rest = tail;
        }
        Ok(Self {
            format,
            placeholders: PhantomData,
        })
    }
}

impl<P: IdPlaceholders> From<IdFormat<P>> for String {
    fn from(format: IdFormat<P>) -> Self {
        format.format
    }
}

impl<P: IdPlaceholders> Display for IdFormat<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format)
    }
}

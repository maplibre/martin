//! CEL expressions over feature properties, compiled once and evaluated per feature:
//!
//! Bare identifiers are property names and `feature['k']` reads keys that are not identifiers.
//! A missing property is `null`.

mod coerce;
mod compile;
mod env;
mod role;
mod sort;
mod view;

use cel::common::types::CelNull;

pub(crate) use crate::expr::compile::CompiledExpr;
pub use crate::expr::compile::ExprError;
pub use crate::expr::role::{Attr, ExprRef, Filter, Id, Role, SortBy, Zoom};
pub use crate::expr::sort::SortValue;
pub(crate) use crate::expr::view::{ExprKeys, FeatureView, PropSlots};

const FEATURE: &str = "feature";

static NULL: CelNull = CelNull;

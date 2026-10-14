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
pub use compile::{CompiledExpr, ExprError};
pub use role::{Attr, ExprRef, Filter, Id, Role, Zoom};
pub use sort::encode_sort;
pub use view::{ExprKeys, FeatureView, PropSlots};

const FEATURE: &str = "feature";

static NULL: CelNull = CelNull;

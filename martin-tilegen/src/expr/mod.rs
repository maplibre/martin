//! CEL expressions over feature properties, compiled once and evaluated per feature:
//!
//! Bare identifiers are property names and `feature['k']` reads keys that are not identifiers.
//! A missing property is `null`.

mod coerce;
mod compile;
mod env;
mod pushdown;
mod role;
mod sort;
mod unparse;
mod view;

use cel::common::ast::{CallExpr, Expr, IdedExpr, operators};
use cel::common::types::CelNull;

pub use crate::expr::compile::ExprError;
pub(crate) use crate::expr::compile::{CompiledExpr, eval_part};
pub(crate) use crate::expr::pushdown::condition_of;
pub use crate::expr::role::{Attr, ExprRef, Filter, Id, Role, SortBy, Zoom};
pub(crate) use crate::expr::unparse::Cel;
pub(crate) use crate::expr::view::{ExprKeys, FeatureView, PropSlots};

pub(crate) const FEATURE: &str = "feature";

/// The key of `call`, when it is `feature[key]`.
pub(crate) fn feature_key(call: &CallExpr) -> Option<&IdedExpr> {
    match (call.func_name.as_str(), call.target.as_deref(), call.args.as_slice()) {
        (operators::INDEX, None, [feature, key]) if matches!(&feature.expr, Expr::Ident(name) if name == FEATURE) => {
            Some(key)
        }
        _ => None,
    }
}

static NULL: CelNull = CelNull;

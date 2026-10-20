use std::borrow::Cow;

use cel::{ExecutionError, Value};

use super::coerce::{to_filter, to_id, to_prop, to_zoom};
use crate::expr::sort::{SortValue, to_sort};
use crate::props::Prop;

/// What an expression is for, and how its result becomes that. A role value carries what is known when
/// the layer is planned, so evaluating needs nothing more.
///
/// `null` and a failed evaluation both count as `Out::default()`: no filter match, and no zoom, id or
/// property.
pub trait Role {
    type Out: Default;

    fn coerce(&self, value: Value) -> Result<Self::Out, ExecutionError>;
}

#[derive(Clone, Copy, Debug)]
pub struct Filter;

#[derive(Clone, Copy, Debug)]
pub struct Zoom;

#[derive(Clone, Copy, Debug)]
pub struct Id;

#[derive(Clone, Copy, Debug)]
pub struct Attr;

#[derive(Clone, Copy, Debug)]
pub struct SortBy {
    pub descending: bool,
}

impl Role for Filter {
    type Out = bool;

    fn coerce(&self, value: Value) -> Result<bool, ExecutionError> {
        to_filter(&value)
    }
}

impl Role for Zoom {
    type Out = Option<u8>;

    fn coerce(&self, value: Value) -> Result<Option<u8>, ExecutionError> {
        to_zoom(&value)
    }
}

impl Role for Id {
    type Out = Option<u64>;

    fn coerce(&self, value: Value) -> Result<Option<u64>, ExecutionError> {
        to_id(&value)
    }
}

impl Role for Attr {
    type Out = Option<Prop<Cow<'static, str>>>;

    fn coerce(&self, value: Value) -> Result<Self::Out, ExecutionError> {
        to_prop(value)
    }
}

impl Role for SortBy {
    type Out = SortValue;

    fn coerce(&self, value: Value) -> Result<SortValue, ExecutionError> {
        to_sort(value)
    }
}

/// Where a compiled expression sits in the plan, and the role it was compiled for.
#[derive(Clone, Copy, Debug)]
pub struct ExprRef<R> {
    pub(crate) index: usize,
    pub(crate) role: R,
}

impl<R: Role> ExprRef<R> {
    pub(crate) fn new(index: usize, role: R) -> Self {
        Self { index, role }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{CompiledExpr, ExprKeys, PropSlots};
    use crate::props::KeyId;

    #[test]
    fn a_filter_over_a_missing_property_drops_the_feature() {
        let expr = CompiledExpr::compile("is_open", &["name", "is_open"], false).unwrap();
        let keys = ExprKeys::columns(&["name", "is_open"]);
        let props = vec![(KeyId::new(0), Prop::Str("Berlin".to_owned()))];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(Filter.coerce(expr.eval(&view).unwrap()), Ok(false));
    }

    #[test]
    fn a_zoom_above_255_is_255() {
        let expr = CompiledExpr::compile("rank * 100", &["rank"], false).unwrap();
        let keys = ExprKeys::columns(&["rank"]);
        let props = vec![(KeyId::new(0), Prop::<String>::I64(3))];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(Zoom.coerce(expr.eval(&view).unwrap()), Ok(Some(255)));
    }
}

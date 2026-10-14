use std::borrow::Cow;
use std::ops::RangeInclusive;

use cel::{ExecutionError, Value};
use martin_core::generate::props::Prop;

use super::{CompiledExpr, ExprError, to_filter, to_id, to_prop, to_zoom};

/// What an expression is for, and how its result becomes that. A role value carries what is known when
/// the layer is planned, so evaluating needs nothing more.
pub trait Role {
    type Out;

    /// `None` leaves the zoom, id or property out. A filter is never `None`.
    fn coerce(&self, value: Value) -> Result<Option<Self::Out>, ExecutionError>;
}

#[derive(Clone, Copy, Debug)]
pub struct Filter;

#[derive(Clone, Copy, Debug)]
pub struct Zoom {
    lo: u8,
    hi: u8,
}

impl Zoom {
    #[must_use]
    pub fn new(range: &RangeInclusive<u8>) -> Self {
        Self {
            lo: *range.start(),
            hi: *range.end(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Id;

#[derive(Clone, Copy, Debug)]
pub struct Attr;

impl Role for Filter {
    type Out = bool;

    fn coerce(&self, value: Value) -> Result<Option<bool>, ExecutionError> {
        to_filter(&value).map(Some)
    }
}

impl Role for Zoom {
    type Out = u8;

    fn coerce(&self, value: Value) -> Result<Option<u8>, ExecutionError> {
        to_zoom(&value, self.lo, self.hi)
    }
}

impl Role for Id {
    type Out = u64;

    fn coerce(&self, value: Value) -> Result<Option<u64>, ExecutionError> {
        to_id(&value)
    }
}

impl Role for Attr {
    type Out = Prop<Cow<'static, str>>;

    fn coerce(&self, value: Value) -> Result<Option<Self::Out>, ExecutionError> {
        to_prop(value)
    }
}

/// An expression compiled for one [`Role`].
#[derive(Debug)]
pub struct TypedExpr<R> {
    expr: CompiledExpr,
    role: R,
}

impl<R: Role> TypedExpr<R> {
    pub fn compile<S: AsRef<str>>(
        source: &str,
        columns: &[S],
        dynamic_props: bool,
        role: R,
    ) -> Result<Self, ExprError> {
        let expr = CompiledExpr::compile(source, columns, dynamic_props)?;
        Ok(Self { expr, role })
    }

    pub fn columns(&self) -> impl Iterator<Item = &str> {
        self.expr.columns()
    }

    #[must_use]
    pub fn needs_all_keys(&self) -> bool {
        self.expr.needs_all_keys()
    }

    pub fn into_parts(self) -> (CompiledExpr, R) {
        (self.expr, self.role)
    }
}

/// Where a compiled expression sits in the plan, and the role it was compiled for.
#[derive(Clone, Copy, Debug)]
pub struct ExprRef<R> {
    pub index: usize,
    pub role: R,
}

#[cfg(test)]
mod tests {
    use martin_core::generate::props::KeyId;

    use super::*;
    use crate::expr::{ExprKeys, PropSlots};

    #[test]
    fn a_filter_over_a_missing_property_drops_the_feature() {
        let (expr, role) = TypedExpr::compile("is_open", &["name", "is_open"], false, Filter)
            .unwrap()
            .into_parts();
        let keys = ExprKeys::columns(&["name", "is_open"]);
        let props = vec![(KeyId::new(0), Prop::Str("Berlin".to_owned()))];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(role.coerce(expr.eval(&view).unwrap()), Ok(Some(false)));
    }

    #[test]
    fn a_zoom_outside_the_layer_range_is_clamped() {
        let (expr, role) = TypedExpr::compile("rank * 10", &["rank"], false, Zoom::new(&(4..=12)))
            .unwrap()
            .into_parts();
        let keys = ExprKeys::columns(&["rank"]);
        let props = vec![(KeyId::new(0), Prop::<String>::I64(3))];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(role.coerce(expr.eval(&view).unwrap()), Ok(Some(12)));
    }
}

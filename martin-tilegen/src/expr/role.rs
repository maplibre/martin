use std::borrow::Cow;
use std::ops::RangeInclusive;

use cel::{ExecutionError, Value};
use martin_core::generate::props::Prop;

use super::coerce::{to_filter, to_id, to_prop, to_zoom};

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

    fn coerce(&self, value: Value) -> Result<bool, ExecutionError> {
        to_filter(&value)
    }
}

impl Role for Zoom {
    type Out = Option<u8>;

    fn coerce(&self, value: Value) -> Result<Option<u8>, ExecutionError> {
        to_zoom(&value, self.lo, self.hi)
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
    use crate::expr::{CompiledExpr, ExprKeys, PropSlots};

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
    fn a_zoom_outside_the_layer_range_is_clamped() {
        let expr = CompiledExpr::compile("rank * 10", &["rank"], false).unwrap();
        let keys = ExprKeys::columns(&["rank"]);
        let props = vec![(KeyId::new(0), Prop::<String>::I64(3))];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(
            Zoom::new(&(4..=12)).coerce(expr.eval(&view).unwrap()),
            Ok(Some(12))
        );
    }
}

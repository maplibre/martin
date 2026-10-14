use std::borrow::Cow;
use std::ops::RangeInclusive;

use cel::{ExecutionError, Value};
use martin_core::generate::props::Prop;

use super::{CompiledExpr, ExprError, to_filter, to_id, to_prop, to_zoom};

/// What an expression is for, and how its result becomes that. A role value carries what is known when
/// the layer is planned, so evaluating needs nothing more.
pub trait Role {
    type Out;

    /// `None` is `null`: the filter, zoom, id or property is left out.
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
        to_zoom(&value, self.lo..=self.hi)
    }
}

impl Role for Id {
    type Out = u64;

    fn coerce(&self, value: Value) -> Result<Option<u64>, ExecutionError> {
        Ok(to_id(&value))
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

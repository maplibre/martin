//! Evaluates the expressions most layers use, property comparisons joined by `&&`, `||`, `!` and `?:`,
//! without the CEL interpreter. Anything it is not sure gives the interpreter's value, it leaves to the
//! interpreter: other operators, mixed types, and every error.

use std::collections::BTreeSet;
use std::sync::Arc;

use cel::Value;
use cel::common::ast::{Expr, IdedExpr, LiteralValue, operators};
use mlt_core::PropValueRef;

use super::{FEATURE, FeatureView, feature_key};

/// An expression of the subset, built from a compiled syntax tree.
#[derive(Debug)]
pub(super) enum FastExpr {
    /// A literal.
    Literal(Scalar),
    /// A bare identifier: the interpreter fails when the table has no such key.
    Ident(Box<str>),
    /// `feature['k']`: `null` when the table has no such key.
    Key(Box<str>),
    /// `!a`.
    Not(Box<Self>),
    /// `a && b`.
    And(Box<[Self; 2]>),
    /// `a || b`.
    Or(Box<[Self; 2]>),
    /// `a op b`.
    Compare(Op, Box<[Self; 2]>),
    /// `a ? b : c`.
    Cond(Box<[Self; 3]>),
}

/// A literal of the subset.
#[derive(Debug)]
pub(super) enum Scalar {
    /// `null`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A signed integer.
    Int(i64),
    /// An unsigned integer.
    UInt(u64),
    /// A double.
    Double(f64),
    /// A string.
    Str(Box<str>),
}

/// A comparison operator.
#[derive(Clone, Copy, Debug)]
pub(super) enum Op {
    /// `==`.
    Eq,
    /// `!=`.
    Ne,
    /// `<`.
    Lt,
    /// `<=`.
    Le,
    /// `>`.
    Gt,
    /// `>=`.
    Ge,
}

/// A value an expression of the subset produces.
#[derive(Clone, Copy, Debug)]
pub(super) enum FastValue<'a> {
    /// `null`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A signed integer.
    Int(i64),
    /// An unsigned integer.
    UInt(u64),
    /// A double.
    Double(f64),
    /// A string.
    Str(&'a str),
}

impl FastExpr {
    /// The subset form of `expr`, whose identifiers are the property names in `columns`; `None` if it
    /// has anything else.
    pub(super) fn new(expr: &IdedExpr, columns: &BTreeSet<String>) -> Option<Self> {
        Some(match &expr.expr {
            Expr::Literal(literal) => Self::Literal(match literal {
                LiteralValue::Null => Scalar::Null,
                LiteralValue::Boolean(v) => Scalar::Bool(*v.inner()),
                LiteralValue::Int(v) => Scalar::Int(*v.inner()),
                LiteralValue::UInt(v) => Scalar::UInt(*v.inner()),
                LiteralValue::Double(v) => Scalar::Double(*v.inner()),
                LiteralValue::String(v) => Scalar::Str(v.inner().into()),
                LiteralValue::Bytes(_) => return None,
            }),
            Expr::Ident(name) if name != FEATURE && columns.contains(name) => Self::Ident(name.as_str().into()),
            Expr::Call(call) => {
                if let Some(key) = feature_key(call) {
                    let Expr::Literal(LiteralValue::String(key)) = &key.expr else {
                        return None;
                    };
                    return Some(Self::Key(key.inner().into()));
                }
                if call.target.is_some() {
                    return None;
                }
                let arg = |i: usize| Self::new(&call.args[i], columns);
                match (call.func_name.as_str(), call.args.len()) {
                    (operators::LOGICAL_NOT, 1) => Self::Not(Box::new(arg(0)?)),
                    (operators::LOGICAL_AND, 2) => Self::And(Box::new([arg(0)?, arg(1)?])),
                    (operators::LOGICAL_OR, 2) => Self::Or(Box::new([arg(0)?, arg(1)?])),
                    (operators::CONDITIONAL, 3) => Self::Cond(Box::new([arg(0)?, arg(1)?, arg(2)?])),
                    (name, 2) => {
                        let op = match name {
                            operators::EQUALS => Op::Eq,
                            operators::NOT_EQUALS => Op::Ne,
                            operators::LESS => Op::Lt,
                            operators::LESS_EQUALS => Op::Le,
                            operators::GREATER => Op::Gt,
                            operators::GREATER_EQUALS => Op::Ge,
                            _ => return None,
                        };
                        Self::Compare(op, Box::new([arg(0)?, arg(1)?]))
                    }
                    _ => return None,
                }
            }
            Expr::Unspecified
            | Expr::Comprehension(_)
            | Expr::Ident(_)
            | Expr::List(_)
            | Expr::Map(_)
            | Expr::Select(_)
            | Expr::Struct(_) => return None,
        })
    }

    /// The value of the expression for `view`, or `None` to leave it to the interpreter.
    pub(super) fn eval<'a, S: AsRef<str>>(&'a self, view: &FeatureView<'a, S>) -> Option<FastValue<'a>> {
        match self {
            Self::Literal(literal) => Some(match literal {
                Scalar::Null => FastValue::Null,
                Scalar::Bool(v) => FastValue::Bool(*v),
                Scalar::Int(v) => FastValue::Int(*v),
                Scalar::UInt(v) => FastValue::UInt(*v),
                Scalar::Double(v) => FastValue::Double(*v),
                Scalar::Str(v) => FastValue::Str(v),
            }),
            Self::Ident(name) => Some(FastValue::of(view.prop(view.key(name)?))),
            Self::Key(name) => Some(FastValue::of(view.key(name).and_then(|key| view.prop(key)))),
            Self::Not(inner) => Some(FastValue::Bool(!inner.eval(view)?.as_bool()?)),
            Self::And(args) => {
                let left = args[0].eval(view)?.as_bool()?;
                let value = !left || args[1].eval(view)?.as_bool()?;
                Some(FastValue::Bool(left && value))
            }
            Self::Or(args) => {
                let left = args[0].eval(view)?.as_bool()?;
                let value = left || args[1].eval(view)?.as_bool()?;
                Some(FastValue::Bool(value))
            }
            Self::Cond(args) => {
                if args[0].eval(view)?.as_bool()? {
                    args[1].eval(view)
                } else {
                    args[2].eval(view)
                }
            }
            Self::Compare(op, args) => {
                let (a, b) = (args[0].eval(view)?, args[1].eval(view)?);
                compare(*op, a, b).map(FastValue::Bool)
            }
        }
    }
}

/// `a op b` where both have the same type, or `==` / `!=` against `null`; `None` otherwise.
fn compare(op: Op, a: FastValue<'_>, b: FastValue<'_>) -> Option<bool> {
    use std::cmp::Ordering;
    let ordering = match (a, b) {
        (FastValue::Null, FastValue::Null) if matches!(op, Op::Eq | Op::Ne) => Ordering::Equal,
        (FastValue::Null, _) | (_, FastValue::Null) => {
            return match op {
                Op::Eq => Some(false),
                Op::Ne => Some(true),
                Op::Lt | Op::Le | Op::Gt | Op::Ge => None,
            };
        }
        (FastValue::Bool(a), FastValue::Bool(b)) if matches!(op, Op::Eq | Op::Ne) => a.cmp(&b),
        (FastValue::Int(a), FastValue::Int(b)) => a.cmp(&b),
        (FastValue::UInt(a), FastValue::UInt(b)) => a.cmp(&b),
        (FastValue::Double(a), FastValue::Double(b)) => a.partial_cmp(&b)?,
        (FastValue::Str(a), FastValue::Str(b)) => a.cmp(b),
        _ => return None,
    };
    Some(match op {
        Op::Eq => ordering.is_eq(),
        Op::Ne => ordering.is_ne(),
        Op::Lt => ordering.is_lt(),
        Op::Le => ordering.is_le(),
        Op::Gt => ordering.is_gt(),
        Op::Ge => ordering.is_ge(),
    })
}

impl FastValue<'_> {
    /// The boolean, or `None` for any other type.
    fn as_bool(self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(v),
            Self::Null | Self::Int(_) | Self::UInt(_) | Self::Double(_) | Self::Str(_) => None,
        }
    }

    /// The value the interpreter sees for a property, as the feature view converts it.
    fn of(prop: Option<PropValueRef<'_>>) -> FastValue<'_> {
        match prop {
            None => FastValue::Null,
            Some(PropValueRef::Bool(v)) => FastValue::Bool(v),
            Some(PropValueRef::I8(v)) => FastValue::Int(i64::from(v)),
            Some(PropValueRef::I32(v)) => FastValue::Int(i64::from(v)),
            Some(PropValueRef::I64(v)) => FastValue::Int(v),
            Some(PropValueRef::U8(v)) => FastValue::UInt(u64::from(v)),
            Some(PropValueRef::U32(v)) => FastValue::UInt(u64::from(v)),
            Some(PropValueRef::U64(v)) => FastValue::UInt(v),
            Some(PropValueRef::F32(v)) => FastValue::Double(f64::from(v)),
            Some(PropValueRef::F64(v)) => FastValue::Double(v),
            Some(PropValueRef::Str(v)) => FastValue::Str(v),
        }
    }
}

impl From<FastValue<'_>> for Value {
    fn from(value: FastValue<'_>) -> Self {
        match value {
            FastValue::Null => Self::Null,
            FastValue::Bool(v) => Self::Bool(v),
            FastValue::Int(v) => Self::Int(v),
            FastValue::UInt(v) => Self::UInt(v),
            FastValue::Double(v) => Self::Float(v),
            FastValue::Str(v) => Self::String(Arc::new(v.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use cel::Value;

    use crate::expr::{CompiledExpr, ExprKeys, PropSlots, eval_part};
    use crate::props::{KeyId, Prop};

    const OPERANDS: [&str; 15] = [
        "a", "b", "missing", "null", "true", "false", "0", "8", "-1", "8u", "0.0", "8.0", "''", "'place'", "'Place'",
    ];

    fn props() -> Vec<Vec<(KeyId, Prop<String>)>> {
        let values = [
            None,
            Some(Prop::Bool(true)),
            Some(Prop::Bool(false)),
            Some(Prop::I64(8)),
            Some(Prop::I64(-1)),
            Some(Prop::F64(8.0)),
            Some(Prop::F32(0.5)),
            Some(Prop::F64(f64::NAN)),
            Some(Prop::Str(String::new())),
            Some(Prop::Str("place".to_owned())),
        ];
        let mut out = Vec::new();
        for a in &values {
            for b in &values {
                let mut props = Vec::new();
                if let Some(a) = a {
                    props.push((KeyId::new(0), a.clone()));
                }
                if let Some(b) = b {
                    props.push((KeyId::new(1), b.clone()));
                }
                out.push(props);
            }
        }
        out
    }

    fn assert_same(source: &str, all: &[Vec<(KeyId, Prop<String>)>]) {
        let columns = ["a", "b", "c"];
        let Ok(expr) = CompiledExpr::compile(source, &columns, false) else {
            return;
        };
        let keys = ExprKeys::columns(&columns);
        let mut slots = PropSlots::default();
        for props in all {
            let view = slots.bind(&keys, props);
            let interpreted = eval_part(expr.ast(), &view);
            match (expr.eval(&view), interpreted) {
                (Ok(fast), Ok(slow)) => assert!(
                    fast == slow
                        || matches!((&fast, &slow), (Value::Float(x), Value::Float(y)) if x.is_nan() && y.is_nan()),
                    "{source} with {props:?}: {fast:?} vs {slow:?}"
                ),
                (Err(_), Err(_)) => {}
                (fast, slow) => panic!("{source} with {props:?}: {fast:?} vs {slow:?}"),
            }
        }
    }

    #[test]
    fn comparisons_match_the_interpreter() {
        let all = props();
        for a in OPERANDS {
            for b in OPERANDS {
                for op in ["==", "!=", "<", "<=", ">", ">="] {
                    assert_same(&format!("{a} {op} {b}"), &all);
                }
            }
        }
    }

    #[test]
    fn logic_matches_the_interpreter() {
        let all = props();
        for a in OPERANDS {
            for b in OPERANDS {
                for source in [
                    format!("{a} && {b}"),
                    format!("{a} || {b}"),
                    format!("!{a}"),
                    format!("{a} ? {b} : 3"),
                    format!("({a} == {b}) && a != null"),
                    format!("a == {a} || b == {b}"),
                    format!("feature['a'] == {b}"),
                ] {
                    assert_same(&source, &all);
                }
            }
        }
    }
}

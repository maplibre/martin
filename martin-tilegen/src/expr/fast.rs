//! Evaluates the expressions most layers use without the CEL interpreter: comparisons of literals,
//! properties and their `int`, `double` or `bool` conversions, joined by `&&`, `||` and `!`, and one
//! `?:` choosing between two operands. Anything it is not sure gives the interpreter's value, it leaves
//! to the interpreter: other operators, mixed types, `?:` chains, and every error.

use std::collections::BTreeSet;
use std::sync::Arc;

use cel::Value;
use cel::common::ast::{Expr, IdedExpr, LiteralValue, operators};
use mlt_core::PropValueRef;

use super::{FEATURE, FeatureView, feature_key};

/// An expression of the subset, built from a compiled syntax tree.
#[derive(Debug)]
pub(super) enum FastExpr {
    /// A condition, such as a `where`.
    Pred(Pred),
    /// An operand on its own, such as an attribute.
    Operand(Operand),
    /// `cond ? a : b` of two operands, such as a `minzoom`.
    Cond(Box<(Pred, Operand, Operand)>),
}

/// A condition, with every `!` folded into its comparisons and operands.
#[derive(Debug)]
pub(super) enum Pred {
    /// `a op b` of two operands.
    Compare(Op, Box<[Operand; 2]>),
    /// An operand that must be `true`, such as a boolean property.
    True(Operand),
    /// An operand that must be `false`, from `!a`.
    False(Operand),
    /// `a && b && ...`, none of whose parts is itself an `All`.
    All(Box<[Self]>),
    /// `a || b || ...`, none of whose parts is itself an `Any`.
    Any(Box<[Self]>),
}

/// A literal, a property, or a converted property.
#[derive(Debug)]
pub(super) enum Operand {
    /// A literal.
    Literal(Scalar),
    /// A property.
    Read(Read),
    /// `int(p)`, `double(p)` or `bool(p)` of a property.
    Convert(Conversion, Read),
}

/// A property read.
#[derive(Debug)]
pub(super) enum Read {
    /// A bare identifier: the interpreter fails when the table has no such key.
    Ident(Box<str>),
    /// `feature['k']`: `null` when the table has no such key.
    Key(Box<str>),
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

/// A comparison operator, parsed from its CEL function name.
#[derive(Clone, Copy, Debug, strum::EnumString)]
pub(super) enum Op {
    /// `==`.
    #[strum(serialize = "_==_")]
    Eq,
    /// `!=`.
    #[strum(serialize = "_!=_")]
    Ne,
    /// `<`.
    #[strum(serialize = "_<_")]
    Lt,
    /// `<=`.
    #[strum(serialize = "_<=_")]
    Le,
    /// `>`.
    #[strum(serialize = "_>_")]
    Gt,
    /// `>=`.
    #[strum(serialize = "_>=_")]
    Ge,
}

/// A CEL conversion function, parsed from its name.
#[derive(Clone, Copy, Debug, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub(super) enum Conversion {
    /// `int(v)`.
    Int,
    /// `double(v)`.
    Double,
    /// `bool(v)`.
    Bool,
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
        if let Expr::Call(call) = &expr.expr
            && call.target.is_none()
            && call.func_name == operators::CONDITIONAL
            && let [cond, then, otherwise] = call.args.as_slice()
        {
            return Some(Self::Cond(Box::new((
                Pred::new(cond, columns)?,
                Operand::new(then, columns)?,
                Operand::new(otherwise, columns)?,
            ))));
        }
        match Pred::call(expr, columns) {
            Some(pred) => Some(Self::Pred(pred)),
            None => Operand::new(expr, columns).map(Self::Operand),
        }
    }

    /// The value of the expression for `view`, or `None` to leave it to the interpreter.
    pub(super) fn eval<'a, S: AsRef<str>>(&'a self, view: &FeatureView<'a, S>) -> Option<FastValue<'a>> {
        match self {
            Self::Pred(pred) => pred.eval(view).map(FastValue::Bool),
            Self::Operand(operand) => operand.eval(view),
            Self::Cond(cond) => {
                let (pred, then, otherwise) = &**cond;
                if pred.eval(view)? {
                    then.eval(view)
                } else {
                    otherwise.eval(view)
                }
            }
        }
    }
}

impl Pred {
    /// The condition `expr` is, also a bare operand, as the parts of `&&`, `||`, `!` and `?:` may be.
    fn new(expr: &IdedExpr, columns: &BTreeSet<String>) -> Option<Self> {
        Self::call(expr, columns).or_else(|| Operand::new(expr, columns).map(Self::True))
    }

    /// The condition `expr` is, if it is `!`, `&&`, `||` or a comparison.
    fn call(expr: &IdedExpr, columns: &BTreeSet<String>) -> Option<Self> {
        let Expr::Call(call) = &expr.expr else {
            return None;
        };
        if call.target.is_some() {
            return None;
        }
        let part = |i: usize| Self::new(&call.args[i], columns);
        let operand = |i: usize| Operand::new(&call.args[i], columns);
        Some(match (call.func_name.as_str(), call.args.len()) {
            (operators::LOGICAL_NOT, 1) => part(0)?.negate(),
            (operators::LOGICAL_AND, 2) => Self::all(part(0)?, part(1)?),
            (operators::LOGICAL_OR, 2) => Self::any(part(0)?, part(1)?),
            (name, 2) => Self::Compare(name.parse().ok()?, Box::new([operand(0)?, operand(1)?])),
            _ => return None,
        })
    }

    /// `a && b`, with the parts of `a` or `b` that are already an `All` merged in.
    fn all(a: Self, b: Self) -> Self {
        let mut parts = Vec::new();
        for pred in [a, b] {
            if let Self::All(inner) = pred {
                parts.extend(inner);
            } else {
                parts.push(pred);
            }
        }
        Self::All(parts.into())
    }

    /// `a || b`, with the parts of `a` or `b` that are already an `Any` merged in.
    fn any(a: Self, b: Self) -> Self {
        let mut parts = Vec::new();
        for pred in [a, b] {
            if let Self::Any(inner) = pred {
                parts.extend(inner);
            } else {
                parts.push(pred);
            }
        }
        Self::Any(parts.into())
    }

    /// `!self`: the opposite comparison, the opposite operand test, or De Morgan's law.
    fn negate(self) -> Self {
        match self {
            Self::Compare(op, args) => Self::Compare(op.negate(), args),
            Self::True(operand) => Self::False(operand),
            Self::False(operand) => Self::True(operand),
            Self::All(parts) => Self::Any(parts.into_iter().map(Self::negate).collect()),
            Self::Any(parts) => Self::All(parts.into_iter().map(Self::negate).collect()),
        }
    }

    /// Whether the condition holds for `view`, or `None` to leave it to the interpreter.
    fn eval<S: AsRef<str>>(&self, view: &FeatureView<'_, S>) -> Option<bool> {
        match self {
            Self::Compare(op, args) => compare(*op, args[0].eval(view)?, args[1].eval(view)?),
            Self::True(operand) => operand.eval(view)?.as_bool(),
            Self::False(operand) => operand.eval(view)?.as_bool().map(|v| !v),
            Self::All(parts) => {
                for part in parts {
                    if !part.eval(view)? {
                        return Some(false);
                    }
                }
                Some(true)
            }
            Self::Any(parts) => {
                for part in parts {
                    if part.eval(view)? {
                        return Some(true);
                    }
                }
                Some(false)
            }
        }
    }
}

impl Operand {
    /// The operand `expr` is, if it is a literal, a property or a conversion of a property.
    fn new(expr: &IdedExpr, columns: &BTreeSet<String>) -> Option<Self> {
        if let Expr::Literal(literal) = &expr.expr {
            return Scalar::new(literal).map(Self::Literal);
        }
        if let Some(read) = Read::new(expr, columns) {
            return Some(Self::Read(read));
        }
        let Expr::Call(call) = &expr.expr else {
            return None;
        };
        match (call.target.as_deref(), call.args.as_slice()) {
            (None, [arg]) => Some(Self::Convert(call.func_name.parse().ok()?, Read::new(arg, columns)?)),
            _ => None,
        }
    }

    /// The value of the operand for `view`, or `None` to leave it to the interpreter.
    fn eval<'a, S: AsRef<str>>(&'a self, view: &FeatureView<'a, S>) -> Option<FastValue<'a>> {
        match self {
            Self::Literal(literal) => Some(literal.value()),
            Self::Read(read) => read.eval(view),
            Self::Convert(conversion, read) => conversion.apply(read.eval(view)?),
        }
    }
}

impl Read {
    /// The property `expr` reads, if it is a column name or `feature['k']`.
    fn new(expr: &IdedExpr, columns: &BTreeSet<String>) -> Option<Self> {
        match &expr.expr {
            Expr::Ident(name) if name != FEATURE && columns.contains(name) => Some(Self::Ident(name.as_str().into())),
            Expr::Call(call) => {
                let Expr::Literal(LiteralValue::String(key)) = &feature_key(call)?.expr else {
                    return None;
                };
                Some(Self::Key(key.inner().into()))
            }
            Expr::Unspecified
            | Expr::Comprehension(_)
            | Expr::Ident(_)
            | Expr::List(_)
            | Expr::Literal(_)
            | Expr::Map(_)
            | Expr::Select(_)
            | Expr::Struct(_) => None,
        }
    }

    /// The property's value for `view`, or `None` when the interpreter would fail.
    fn eval<'a, S: AsRef<str>>(&self, view: &FeatureView<'a, S>) -> Option<FastValue<'a>> {
        match self {
            Self::Ident(name) => Some(FastValue::of(view.prop(view.key(name)?))),
            Self::Key(name) => Some(FastValue::of(view.key(name).and_then(|key| view.prop(key)))),
        }
    }
}

impl Scalar {
    /// The literal, if it is not bytes.
    fn new(literal: &LiteralValue) -> Option<Self> {
        Some(match literal {
            LiteralValue::Null => Self::Null,
            LiteralValue::Boolean(v) => Self::Bool(*v.inner()),
            LiteralValue::Int(v) => Self::Int(*v.inner()),
            LiteralValue::UInt(v) => Self::UInt(*v.inner()),
            LiteralValue::Double(v) => Self::Double(*v.inner()),
            LiteralValue::String(v) => Self::Str(v.inner().into()),
            LiteralValue::Bytes(_) => return None,
        })
    }

    /// The literal as a value.
    fn value(&self) -> FastValue<'_> {
        match self {
            Self::Null => FastValue::Null,
            Self::Bool(v) => FastValue::Bool(*v),
            Self::Int(v) => FastValue::Int(*v),
            Self::UInt(v) => FastValue::UInt(*v),
            Self::Double(v) => FastValue::Double(*v),
            Self::Str(v) => FastValue::Str(v),
        }
    }
}

impl Op {
    /// The operator that holds exactly when this one does not, for two values of one type.
    fn negate(self) -> Self {
        match self {
            Self::Eq => Self::Ne,
            Self::Ne => Self::Eq,
            Self::Lt => Self::Ge,
            Self::Le => Self::Gt,
            Self::Gt => Self::Le,
            Self::Ge => Self::Lt,
        }
    }
}

impl Conversion {
    /// `value` converted as the CEL function converts it, or `None` where that fails.
    fn apply(self, value: FastValue<'_>) -> Option<FastValue<'static>> {
        Some(match (self, value) {
            (Self::Int, FastValue::Int(v)) => FastValue::Int(v),
            (Self::Int, FastValue::UInt(v)) => FastValue::Int(i64::try_from(v).ok()?),
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_precision_loss,
                reason = "cel's int() truncates doubles strictly inside the i64 range"
            )]
            (Self::Int, FastValue::Double(v)) if v > i64::MIN as f64 && v < i64::MAX as f64 => FastValue::Int(v as i64),
            (Self::Int, FastValue::Str(v)) => FastValue::Int(v.parse().ok()?),
            #[expect(clippy::cast_precision_loss, reason = "cel's double() rounds integers the same way")]
            (Self::Double, FastValue::Int(v)) => FastValue::Double(v as f64),
            #[expect(clippy::cast_precision_loss, reason = "cel's double() rounds integers the same way")]
            (Self::Double, FastValue::UInt(v)) => FastValue::Double(v as f64),
            (Self::Double, FastValue::Double(v)) => FastValue::Double(v),
            (Self::Double, FastValue::Str(v)) => FastValue::Double(v.parse().ok()?),
            (Self::Bool, FastValue::Bool(v)) => FastValue::Bool(v),
            (Self::Bool, FastValue::Str("1" | "t" | "true" | "TRUE" | "True")) => FastValue::Bool(true),
            (Self::Bool, FastValue::Str("0" | "f" | "false" | "FALSE" | "False")) => FastValue::Bool(false),
            _ => return None,
        })
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
    use rstest::rstest;

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
            Some(Prop::Str("8".to_owned())),
            Some(Prop::Str("-1".to_owned())),
            Some(Prop::Str("0.5".to_owned())),
            Some(Prop::Str("1e3".to_owned())),
            Some(Prop::Str("t".to_owned())),
            Some(Prop::Str("False".to_owned())),
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

    #[rstest]
    fn comparisons_match_the_interpreter(#[values("==", "!=", "<", "<=", ">", ">=")] op: &str) {
        let all = props();
        for a in OPERANDS {
            for b in OPERANDS {
                assert_same(&format!("{a} {op} {b}"), &all);
            }
        }
    }

    #[rstest]
    #[case::and("{a} && {b}")]
    #[case::or("{a} || {b}")]
    #[case::not("!{a}")]
    #[case::not_twice("!!{a}")]
    #[case::not_of_a_comparison("!({a} < {b})")]
    #[case::not_of_or("!({a} || {b})")]
    #[case::not_of_and("!(a == {a} && b == {b})")]
    #[case::or_of_three("a == {a} || b == {b} || a == {b}")]
    #[case::and_of_ors("(a == {a} || b == {b}) && (a != {b} || b != {a})")]
    #[case::comparison_and_null_check("({a} == {b}) && a != null")]
    #[case::key_comparison("feature['a'] == {b}")]
    #[case::choice("{a} ? {b} : 3")]
    fn logic_matches_the_interpreter(#[case] shape: &str) {
        let all = props();
        for a in OPERANDS {
            for b in OPERANDS {
                assert_same(&shape.replace("{a}", a).replace("{b}", b), &all);
            }
        }
    }

    #[rstest]
    fn conversions_match_the_interpreter(
        #[values("int", "double", "bool")] conversion: &str,
        #[values("a", "feature['a']", "feature['missing']")] arg: &str,
        #[values("==", "!=", "<", "<=", ">", ">=")] op: &str,
    ) {
        let all = props();
        let converted = format!("{conversion}({arg})");
        assert_same(&converted, &all);
        for b in OPERANDS {
            assert_same(&format!("{converted} {op} {b}"), &all);
        }
    }

    #[rstest]
    #[case::int_of_digits("int(a)", "12", Some(Value::Int(12)))]
    #[case::int_of_a_signed_number("int(a)", "-3", Some(Value::Int(-3)))]
    #[case::int_of_a_decimal_fails("int(a)", "1.5", None)]
    #[case::int_of_padded_digits_fails("int(a)", " 12", None)]
    #[case::double_of_a_decimal("double(a)", "1.5", Some(Value::Float(1.5)))]
    #[case::double_of_an_exponent("double(a)", "1e3", Some(Value::Float(1000.0)))]
    #[case::double_of_a_word_fails("double(a)", "abc", None)]
    #[case::bool_of_t("bool(a)", "t", Some(Value::Bool(true)))]
    #[case::bool_of_capitalized_false("bool(a)", "False", Some(Value::Bool(false)))]
    #[case::bool_of_yes_fails("bool(a)", "yes", None)]
    fn conversions_read_strings_as_cel_does(
        #[case] source: &str,
        #[case] value: &str,
        #[case] expected: Option<Value>,
    ) {
        let columns = ["a"];
        let expr = CompiledExpr::compile(source, &columns, false).unwrap();
        let keys = ExprKeys::columns(&columns);
        let props = vec![(KeyId::new(0), Prop::Str(value.to_owned()))];
        let mut slots = PropSlots::default();
        let view = slots.bind(&keys, &props);
        assert_eq!(expr.eval(&view).ok(), expected);
    }
}

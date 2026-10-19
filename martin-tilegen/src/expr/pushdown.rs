//! Translates a filter expression into a [`Condition`] true for exactly the features it keeps.

use cel::common::ast::{CallExpr, Expr, IdedExpr, LiteralValue, operators};

use super::{CompiledExpr, FEATURE};
use crate::plan::{ColumnDef, CompareOp, Condition, Literal, Term, TranslateError, ValueKind};

impl CompiledExpr {
    /// The rows this filter keeps, reading only `columns`.
    pub(crate) fn condition(&self, columns: &[ColumnDef]) -> Result<Condition, TranslateError> {
        Translator { columns }.condition(&self.expr, false)
    }
}

/// Translates expressions over a table's columns, pushing negations down to the comparisons.
struct Translator<'a> {
    columns: &'a [ColumnDef],
}

/// Where an operand's value comes from, which decides how it compares with `null`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Origin {
    Literal,
    Null,
    /// `null` where the row has none, and `null == null`.
    Column,
}

/// One side of a comparison.
struct Operand {
    term: Term,
    kind: Option<ValueKind>,
    origin: Origin,
}

impl Operand {
    /// A literal of `kind`.
    fn literal(literal: Literal, kind: ValueKind) -> Self {
        Self {
            term: Term::Literal(literal),
            kind: Some(kind),
            origin: Origin::Literal,
        }
    }
}

impl Translator<'_> {
    /// `expr` as a condition, negated if `negated`.
    fn condition(&self, expr: &IdedExpr, negated: bool) -> Result<Condition, TranslateError> {
        match &expr.expr {
            Expr::Literal(LiteralValue::Boolean(value)) => {
                Ok(Condition::Const(*value.inner() != negated))
            }
            Expr::Call(call) => self.call(call, negated),
            Expr::Unspecified
            | Expr::Ident(_)
            | Expr::Select(_)
            | Expr::Literal(_)
            | Expr::Comprehension(_)
            | Expr::List(_)
            | Expr::Map(_)
            | Expr::Struct(_) => is_true(self.operand(expr)?, negated),
        }
    }

    /// A call as a condition, negated if `negated`.
    fn call(&self, call: &CallExpr, negated: bool) -> Result<Condition, TranslateError> {
        let name = call.func_name.as_str();
        let combine = |all: bool, parts: [Condition; 2]| {
            if all == negated {
                Condition::or(parts)
            } else {
                Condition::and(parts)
            }
        };
        match (name, call.target.as_deref(), call.args.as_slice()) {
            (operators::LOGICAL_NOT, None, [arg]) => self.condition(arg, !negated),
            (operators::LOGICAL_AND, None, [left, right]) => Ok(combine(
                true,
                [
                    self.condition(left, negated)?,
                    self.condition(right, negated)?,
                ],
            )),
            (operators::LOGICAL_OR, None, [left, right]) => Ok(combine(
                false,
                [
                    self.condition(left, negated)?,
                    self.condition(right, negated)?,
                ],
            )),
            (operators::CONDITIONAL, None, [test, then, otherwise]) => Ok(Condition::or([
                Condition::and([self.condition(test, false)?, self.condition(then, negated)?]),
                Condition::and([
                    self.condition(test, true)?,
                    self.condition(otherwise, negated)?,
                ]),
            ])),
            (operators::EQUALS, None, [left, right]) => {
                equals(self.operand(left)?, self.operand(right)?, !negated)
            }
            (operators::NOT_EQUALS, None, [left, right]) => {
                equals(self.operand(left)?, self.operand(right)?, negated)
            }
            (operators::LESS, None, [left, right]) => {
                self.order(left, CompareOp::Lt, right, negated)
            }
            (operators::LESS_EQUALS, None, [left, right]) => {
                self.order(left, CompareOp::Le, right, negated)
            }
            (operators::GREATER, None, [left, right]) => {
                self.order(left, CompareOp::Gt, right, negated)
            }
            (operators::GREATER_EQUALS, None, [left, right]) => {
                self.order(left, CompareOp::Ge, right, negated)
            }
            _ => is_true(Self::call_operand(call)?, negated),
        }
    }

    /// `left op right` for an ordering `op`, negated if `negated`.
    fn order(
        &self,
        left: &IdedExpr,
        op: CompareOp,
        right: &IdedExpr,
        negated: bool,
    ) -> Result<Condition, TranslateError> {
        let (left, right) = (self.operand(left)?, self.operand(right)?);
        let (operand, op, literal) = match (left.origin, right.origin) {
            (Origin::Column, Origin::Literal) => (left, op, right),
            (Origin::Literal, Origin::Column) => (right, op.mirror(), left),
            _ => return Err(TranslateError::Order),
        };
        let op = if negated { op.negate() } else { op };
        if operand.kind.is_none() {
            return Err(unknown_kind(&operand.term));
        }
        let Some(kind) = common_kind(operand.kind, literal.kind) else {
            return Ok(Condition::Const(false));
        };
        let not_nan = (operand.kind == Some(ValueKind::Float)).then(|| Condition::Compare {
            left: operand.term.clone(),
            op: CompareOp::Ne,
            right: Term::Literal(Literal::Float(f64::NAN)),
            kind: ValueKind::Float,
        });
        let compare = Condition::Compare {
            left: operand.term,
            op,
            right: literal.term,
            kind,
        };
        Ok(Condition::and(std::iter::once(compare).chain(not_nan)))
    }

    /// `expr` as one side of a comparison.
    fn operand(&self, expr: &IdedExpr) -> Result<Operand, TranslateError> {
        match &expr.expr {
            Expr::Ident(name) if name != FEATURE => self.column(name),
            Expr::Select(_) => Err(TranslateError::SelectsField),
            Expr::Literal(literal) => literal_operand(literal),
            Expr::Call(call) => Self::call_operand(call),
            Expr::Unspecified
            | Expr::Ident(_)
            | Expr::Comprehension(_)
            | Expr::List(_)
            | Expr::Map(_)
            | Expr::Struct(_) => Err(TranslateError::Expression),
        }
    }

    /// A call as one side of a comparison: a negative literal.
    fn call_operand(call: &CallExpr) -> Result<Operand, TranslateError> {
        let name = call.func_name.as_str();
        if name == operators::NEGATE
            && call.target.is_none()
            && let [arg] = call.args.as_slice()
            && let Expr::Literal(literal) = &arg.expr
        {
            return negative_literal(literal);
        }
        let shown = name.trim_matches('_').trim_start_matches('@');
        Err(TranslateError::Function(shown.to_owned()))
    }

    /// The column named `name`, with the kind of its values if it is known.
    fn column(&self, name: &str) -> Result<Operand, TranslateError> {
        let kind = self
            .columns
            .iter()
            .find_map(|column| match column {
                ColumnDef::Typed(column, kind) if column == name => Some(Some(*kind)),
                ColumnDef::Value(column) if column == name => Some(None),
                ColumnDef::Typed(..) | ColumnDef::Value(_) | ColumnDef::Keys(_) => None,
            })
            .ok_or_else(|| TranslateError::NotAColumn(name.to_owned()))?;
        Ok(Operand {
            term: Term::Column(name.to_owned()),
            kind,
            origin: Origin::Column,
        })
    }
}

/// A value as a filter: only `true` passes, or only `false` once negated.
fn is_true(operand: Operand, negated: bool) -> Result<Condition, TranslateError> {
    match (operand.origin, operand.kind) {
        (Origin::Column, Some(ValueKind::Bool)) => Ok(Condition::Compare {
            left: operand.term,
            op: CompareOp::Eq,
            right: Term::Literal(Literal::Bool(!negated)),
            kind: ValueKind::Bool,
        }),
        (Origin::Column, None) => Err(unknown_kind(&operand.term)),
        _ => Ok(Condition::Const(false)),
    }
}

/// `left == right` if `equal`, else `left != right`.
fn equals(left: Operand, right: Operand, equal: bool) -> Result<Condition, TranslateError> {
    let (left, right) = if left.origin < right.origin {
        (right, left)
    } else {
        (left, right)
    };
    let kind = common_kind(left.kind, right.kind);
    Ok(match (left.origin, right.origin, kind) {
        (Origin::Literal | Origin::Null, _, _) => return Err(TranslateError::Constants),
        (Origin::Column, Origin::Null, _) => Condition::IsNull {
            term: left.term,
            negated: !equal,
        },
        (Origin::Column, _, _) if left.kind.is_none() || right.kind.is_none() => {
            return Err(unknown_kind(if left.kind.is_none() {
                &left.term
            } else {
                &right.term
            }));
        }
        (Origin::Column, Origin::Literal, None) => Condition::Const(!equal),
        (Origin::Column, Origin::Literal, Some(kind)) if equal => Condition::Compare {
            left: left.term,
            op: CompareOp::Eq,
            right: right.term,
            kind,
        },
        (Origin::Column, Origin::Column, Some(ValueKind::Float)) if left.kind == right.kind => {
            return Err(TranslateError::FloatColumns);
        }
        (Origin::Column, Origin::Literal | Origin::Column, Some(kind)) => Condition::Distinct {
            left: left.term,
            right: right.term,
            kind,
            negated: right.origin == Origin::Column && equal,
        },
        (Origin::Column, Origin::Column, None) => {
            let parts = [left.term, right.term].map(|term| Condition::IsNull {
                term,
                negated: !equal,
            });
            if equal {
                Condition::and(parts)
            } else {
                Condition::or(parts)
            }
        }
    })
}

/// The type two values compare as: numbers as floats once one is a float.
fn common_kind(left: Option<ValueKind>, right: Option<ValueKind>) -> Option<ValueKind> {
    match (left?, right?) {
        (ValueKind::Float, ValueKind::Int) | (ValueKind::Int, ValueKind::Float) => {
            Some(ValueKind::Float)
        }
        (left, right) => (left == right).then_some(left),
    }
}

/// The error for comparing `term`, whose type is not known.
fn unknown_kind(term: &Term) -> TranslateError {
    match term {
        Term::Column(name) => TranslateError::UnknownColumnType(name.clone()),
        Term::Literal(_) => TranslateError::UnknownType,
    }
}

/// A literal as an operand.
fn literal_operand(literal: &LiteralValue) -> Result<Operand, TranslateError> {
    Ok(match literal {
        LiteralValue::Null => Operand {
            term: Term::Literal(Literal::Bool(false)),
            kind: None,
            origin: Origin::Null,
        },
        LiteralValue::Boolean(v) => Operand::literal(Literal::Bool(*v.inner()), ValueKind::Bool),
        LiteralValue::Int(v) => Operand::literal(Literal::Int(*v.inner()), ValueKind::Int),
        LiteralValue::UInt(v) => Operand::literal(Literal::UInt(*v.inner()), ValueKind::Int),
        LiteralValue::Double(v) => Operand::literal(Literal::Float(*v.inner()), ValueKind::Float),
        LiteralValue::String(v) if v.inner().contains('\0') => {
            return Err(TranslateError::NulInString);
        }
        LiteralValue::String(v) => {
            Operand::literal(Literal::String(v.inner().to_owned()), ValueKind::String)
        }
        LiteralValue::Bytes(_) => return Err(TranslateError::Bytes),
    })
}

/// A negated number literal as an operand.
fn negative_literal(literal: &LiteralValue) -> Result<Operand, TranslateError> {
    match literal {
        LiteralValue::Int(v) => {
            let negated = v
                .inner()
                .checked_neg()
                .ok_or(TranslateError::NegatedMinimum)?;
            Ok(Operand::literal(Literal::Int(negated), ValueKind::Int))
        }
        LiteralValue::Double(v) => Ok(Operand::literal(
            Literal::Float(-v.inner()),
            ValueKind::Float,
        )),
        LiteralValue::Boolean(_)
        | LiteralValue::Bytes(_)
        | LiteralValue::Null
        | LiteralValue::String(_)
        | LiteralValue::UInt(_) => Err(TranslateError::NegatedNonNumber),
    }
}

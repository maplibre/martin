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

/// One side of a comparison.
enum Operand {
    /// The `null` literal.
    Null,
    /// A literal other than `null`, of `kind`.
    Literal(Literal, ValueKind),
    /// A column, with the kind of its values if it is known.
    /// It is `null` where the row has no value.
    Column(String, Option<ValueKind>),
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
                equals(self.operand(left)?, self.operand(right)?, negated)
            }
            (operators::NOT_EQUALS, None, [left, right]) => {
                equals(self.operand(left)?, self.operand(right)?, !negated)
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
        let ((name, column_kind), op, (literal, literal_kind)) =
            match (self.operand(left)?, self.operand(right)?) {
                (Operand::Column(name, kind), Operand::Literal(literal, literal_kind)) => {
                    ((name, kind), op, (literal, literal_kind))
                }
                (Operand::Literal(literal, literal_kind), Operand::Column(name, kind)) => {
                    ((name, kind), op.mirror(), (literal, literal_kind))
                }
                _ => return Err(TranslateError::Order),
            };
        let op = if negated { op.negate() } else { op };
        let column_kind = known_kind(&name, column_kind)?;
        let Some(kind) = common_kind(column_kind, literal_kind) else {
            return Ok(Condition::Const(false));
        };
        let column = Term::Column(name);
        let not_nan = (column_kind == ValueKind::Float).then(|| Condition::Compare {
            left: column.clone(),
            op: CompareOp::Ne,
            right: Term::Literal(Literal::Float(f64::NAN)),
            kind: ValueKind::Float,
        });
        let compare = Condition::Compare {
            left: column,
            op,
            right: Term::Literal(literal),
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
        Ok(Operand::Column(name.to_owned(), kind))
    }
}

/// A value as a filter: only `true` passes, or only `false` once negated.
fn is_true(operand: Operand, negated: bool) -> Result<Condition, TranslateError> {
    match operand {
        Operand::Column(name, Some(ValueKind::Bool)) => Ok(Condition::Compare {
            left: Term::Column(name),
            op: CompareOp::Eq,
            right: Term::Literal(Literal::Bool(!negated)),
            kind: ValueKind::Bool,
        }),
        Operand::Column(name, None) => Err(TranslateError::UnknownColumnType(name)),
        Operand::Column(_, Some(_)) | Operand::Literal(..) | Operand::Null => {
            Ok(Condition::Const(false))
        }
    }
}

/// `left == right`, or `left != right` if `negated`.
fn equals(left: Operand, right: Operand, negated: bool) -> Result<Condition, TranslateError> {
    let ((name, kind), other) = match (left, right) {
        (Operand::Column(name, kind), other) | (other, Operand::Column(name, kind)) => {
            ((name, kind), other)
        }
        _ => return Err(TranslateError::Constants),
    };
    match other {
        Operand::Null => Ok(Condition::IsNull {
            term: Term::Column(name),
            negated,
        }),
        Operand::Literal(literal, literal_kind) => {
            let kind = known_kind(&name, kind)?;
            let (column, literal) = (Term::Column(name), Term::Literal(literal));
            Ok(match common_kind(kind, literal_kind) {
                None => Condition::Const(negated),
                Some(kind) if negated => Condition::Distinct {
                    left: column,
                    right: literal,
                    kind,
                    negated: false,
                },
                Some(kind) => Condition::Compare {
                    left: column,
                    op: CompareOp::Eq,
                    right: literal,
                    kind,
                },
            })
        }
        Operand::Column(other, other_kind) => {
            let kind = known_kind(&name, kind)?;
            let other_kind = known_kind(&other, other_kind)?;
            let (left, right) = (Term::Column(name), Term::Column(other));
            match common_kind(kind, other_kind) {
                Some(ValueKind::Float) if kind == other_kind => Err(TranslateError::FloatColumns),
                // CEL holds `null == null`, so two columns compare like `IS NOT DISTINCT FROM`.
                Some(kind) => Ok(Condition::Distinct {
                    left,
                    right,
                    kind,
                    negated: !negated,
                }),
                None => {
                    let parts = [left, right].map(|term| Condition::IsNull { term, negated });
                    Ok(if negated {
                        Condition::or(parts)
                    } else {
                        Condition::and(parts)
                    })
                }
            }
        }
    }
}

/// The kind of column `name`, an error if it is not known.
fn known_kind(name: &str, kind: Option<ValueKind>) -> Result<ValueKind, TranslateError> {
    kind.ok_or_else(|| TranslateError::UnknownColumnType(name.to_owned()))
}

/// The type two values compare as: numbers as floats once one is a float.
fn common_kind(left: ValueKind, right: ValueKind) -> Option<ValueKind> {
    match (left, right) {
        (ValueKind::Float, ValueKind::Int) | (ValueKind::Int, ValueKind::Float) => {
            Some(ValueKind::Float)
        }
        (left, right) => (left == right).then_some(left),
    }
}

/// A literal as an operand.
fn literal_operand(literal: &LiteralValue) -> Result<Operand, TranslateError> {
    Ok(match literal {
        LiteralValue::Null => Operand::Null,
        LiteralValue::Boolean(v) => Operand::Literal(Literal::Bool(*v.inner()), ValueKind::Bool),
        LiteralValue::Int(v) => Operand::Literal(Literal::Int(*v.inner()), ValueKind::Int),
        LiteralValue::UInt(v) => Operand::Literal(Literal::UInt(*v.inner()), ValueKind::Int),
        LiteralValue::Double(v) => Operand::Literal(Literal::Float(*v.inner()), ValueKind::Float),
        LiteralValue::String(v) if v.inner().contains('\0') => {
            return Err(TranslateError::NulInString);
        }
        LiteralValue::String(v) => {
            Operand::Literal(Literal::String(v.inner().to_owned()), ValueKind::String)
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
            Ok(Operand::Literal(Literal::Int(negated), ValueKind::Int))
        }
        LiteralValue::Double(v) => Ok(Operand::Literal(
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

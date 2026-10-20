//! Translates a filter expression into a [`Condition`] true for exactly the features it keeps.

use cel::common::ast::{CallExpr, Expr, IdedExpr, LiteralValue, operators};

use super::{CompiledExpr, FEATURE};
use crate::plan::{ColumnDef, CompareOp, Condition, Literal, Polarity, Term, TranslateError, ValueKind};

impl CompiledExpr {
    /// The rows this filter keeps, reading only `columns`.
    pub(crate) fn condition(&self, columns: &[ColumnDef]) -> Result<Condition, TranslateError> {
        Translator { columns }.condition(&self.expr, Polarity::AsWritten)
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
    /// `expr` as a condition of `polarity`.
    fn condition(&self, expr: &IdedExpr, polarity: Polarity) -> Result<Condition, TranslateError> {
        match &expr.expr {
            Expr::Literal(LiteralValue::Boolean(value)) => Ok(Condition::Const(*value.inner() != polarity.negated())),
            Expr::Call(call) => self.call(call, polarity),
            Expr::Unspecified
            | Expr::Ident(_)
            | Expr::Select(_)
            | Expr::Literal(_)
            | Expr::Comprehension(_)
            | Expr::List(_)
            | Expr::Map(_)
            | Expr::Struct(_) => self.operand(expr)?.into_condition(polarity),
        }
    }

    /// A call as a condition of `polarity`.
    fn call(&self, call: &CallExpr, polarity: Polarity) -> Result<Condition, TranslateError> {
        let name = call.func_name.as_str();
        match (name, call.target.as_deref(), call.args.as_slice()) {
            (operators::LOGICAL_NOT, None, [arg]) => self.condition(arg, polarity.flip()),
            (operators::LOGICAL_AND, None, [left, right]) => {
                Ok(polarity.all([self.condition(left, polarity)?, self.condition(right, polarity)?]))
            }
            (operators::LOGICAL_OR, None, [left, right]) => {
                Ok(polarity.any([self.condition(left, polarity)?, self.condition(right, polarity)?]))
            }
            (operators::CONDITIONAL, None, [test, then, otherwise]) => Ok(Condition::or([
                Condition::and([
                    self.condition(test, Polarity::AsWritten)?,
                    self.condition(then, polarity)?,
                ]),
                Condition::and([
                    self.condition(test, Polarity::Negated)?,
                    self.condition(otherwise, polarity)?,
                ]),
            ])),
            (name, None, [left, right]) if let Some(op) = CompareOp::cel(name) => {
                let (left, right) = (self.operand(left)?, self.operand(right)?);
                match polarity.op(op) {
                    CompareOp::Eq => left.equals(right, Polarity::AsWritten),
                    CompareOp::Ne => left.equals(right, Polarity::Negated),
                    op @ (CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge) => left.order(op, right),
                }
            }
            (operators::IN, None, [key, feature]) if matches!(&feature.expr, Expr::Ident(name) if name == FEATURE) => {
                let name = string_literal(key).ok_or(TranslateError::ComputedKey)?;
                self.column_kind(name)?;
                Ok(Condition::IsNull {
                    term: Term::Column(name.to_owned()),
                    polarity: polarity.flip(),
                })
            }
            (operators::IN, None, [value, list]) => self.in_list(value, list, polarity),
            _ => Operand::call(call)?.into_condition(polarity),
        }
    }

    /// `value in list` of `polarity`, each element compared as it compares with `value`.
    fn in_list(&self, value: &IdedExpr, list: &IdedExpr, polarity: Polarity) -> Result<Condition, TranslateError> {
        let Operand::Column(name, kind) = self.operand(value)? else {
            return Err(TranslateError::ConstantInList);
        };
        let kind = known_kind(&name, kind)?;
        let Expr::List(list) = &list.expr else {
            return Err(TranslateError::NotALiteralList);
        };
        let mut has_null = false;
        let mut lists: Vec<(ValueKind, Vec<Literal>)> = Vec::new();
        for element in &list.elements {
            match self.operand(element)? {
                Operand::Null => has_null = true,
                Operand::Literal(literal, literal_kind) => {
                    let Some(kind) = kind.common(literal_kind) else {
                        continue;
                    };
                    match lists.iter_mut().find(|(listed, _)| *listed == kind) {
                        Some((_, values)) => values.push(literal),
                        None => lists.push((kind, vec![literal])),
                    }
                }
                Operand::Column(..) => return Err(TranslateError::NotALiteralList),
            }
        }
        let negated = polarity.negated();
        let column = Term::Column(name);
        let null_part = (has_null != negated).then(|| Condition::IsNull {
            term: column.clone(),
            polarity: Polarity::AsWritten,
        });
        let present = if lists.is_empty() {
            if negated {
                Condition::IsNull {
                    term: column,
                    polarity: Polarity::Negated,
                }
            } else {
                Condition::Const(false)
            }
        } else {
            polarity.any(lists.into_iter().map(|(kind, values)| Condition::In {
                term: column.clone(),
                values,
                kind,
                polarity,
            }))
        };
        Ok(Condition::or(std::iter::once(present).chain(null_part)))
    }

    /// `expr` as one side of a comparison.
    fn operand(&self, expr: &IdedExpr) -> Result<Operand, TranslateError> {
        match &expr.expr {
            Expr::Ident(name) if name != FEATURE => self.column(name),
            Expr::Select(_) => {
                let name = qualified_name(expr).ok_or(TranslateError::SelectsField)?;
                self.column(&name)
            }
            Expr::Literal(literal) => Operand::literal(literal),
            Expr::Call(call) => {
                if call.func_name == operators::INDEX
                    && let [feature, key] = call.args.as_slice()
                    && matches!(&feature.expr, Expr::Ident(name) if name == FEATURE)
                {
                    let name = string_literal(key).ok_or(TranslateError::ComputedKey)?;
                    return self.column(name);
                }
                Operand::call(call)
            }
            Expr::Ident(_) => Err(TranslateError::Expression("`feature` as a value")),
            Expr::Comprehension(_) => Err(TranslateError::Expression("a comprehension")),
            Expr::List(_) => Err(TranslateError::Expression("a list")),
            Expr::Map(_) => Err(TranslateError::Expression("a map")),
            Expr::Struct(_) => Err(TranslateError::Expression("a message")),
            Expr::Unspecified => Err(TranslateError::Expression("an empty expression")),
        }
    }

    /// The column named `name`, with the kind of its values if it is known.
    fn column(&self, name: &str) -> Result<Operand, TranslateError> {
        let kind = self.column_kind(name)?;
        Ok(Operand::Column(name.to_owned(), kind))
    }

    /// The kind of the values of column `name` if it is known, an error if there is no such column.
    fn column_kind(&self, name: &str) -> Result<Option<ValueKind>, TranslateError> {
        self.columns
            .iter()
            .find_map(|column| match column {
                ColumnDef::Value(column, kind) if column == name => Some(*kind),
                ColumnDef::Value(..) | ColumnDef::Keys(_) => None,
            })
            .ok_or_else(|| TranslateError::NotAColumn(name.to_owned()))
    }
}

impl Operand {
    /// A value as a filter: only `true` passes, or only `false` once negated.
    fn into_condition(self, polarity: Polarity) -> Result<Condition, TranslateError> {
        match self {
            Self::Column(name, Some(ValueKind::Bool)) => Ok(Condition::Compare {
                left: Term::Column(name),
                op: CompareOp::Eq,
                right: Term::Literal(Literal::Bool(!polarity.negated())),
                kind: ValueKind::Bool,
            }),
            Self::Column(name, None) => Err(TranslateError::UnknownColumnType(name)),
            Self::Column(_, Some(_)) | Self::Literal(..) | Self::Null => Ok(Condition::Const(false)),
        }
    }

    /// `self == right`, or `self != right` once negated.
    fn equals(self, right: Self, polarity: Polarity) -> Result<Condition, TranslateError> {
        let ((name, kind), other) = match (self, right) {
            (Self::Column(name, kind), other) | (other, Self::Column(name, kind)) => ((name, kind), other),
            _ => return Err(TranslateError::Constants),
        };
        match other {
            Self::Null => Ok(Condition::IsNull {
                term: Term::Column(name),
                polarity,
            }),
            Self::Literal(literal, literal_kind) => {
                let kind = known_kind(&name, kind)?;
                let (column, literal) = (Term::Column(name), Term::Literal(literal));
                Ok(match kind.common(literal_kind) {
                    None => Condition::Const(polarity.negated()),
                    Some(kind) if polarity.negated() => Condition::Distinct {
                        left: column,
                        right: literal,
                        kind,
                        polarity: Polarity::AsWritten,
                    },
                    Some(kind) => Condition::Compare {
                        left: column,
                        op: CompareOp::Eq,
                        right: literal,
                        kind,
                    },
                })
            }
            Self::Column(other, other_kind) => {
                let kind = known_kind(&name, kind)?;
                let other_kind = known_kind(&other, other_kind)?;
                let (left, right) = (Term::Column(name), Term::Column(other));
                match kind.common(other_kind) {
                    Some(ValueKind::Float) if kind == other_kind => Err(TranslateError::FloatColumns),
                    // CEL holds `null == null`, so two columns compare like `IS NOT DISTINCT FROM`.
                    Some(kind) => Ok(Condition::Distinct {
                        left,
                        right,
                        kind,
                        polarity: polarity.flip(),
                    }),
                    None => Ok(polarity.all([left, right].map(|term| Condition::IsNull { term, polarity }))),
                }
            }
        }
    }

    /// `self op right` for an ordering `op`.
    fn order(self, op: CompareOp, right: Self) -> Result<Condition, TranslateError> {
        let ((name, column_kind), op, (literal, literal_kind)) = match (self, right) {
            (Self::Column(name, kind), Self::Literal(literal, literal_kind)) => {
                ((name, kind), op, (literal, literal_kind))
            }
            (Self::Literal(literal, literal_kind), Self::Column(name, kind)) => {
                ((name, kind), op.mirror(), (literal, literal_kind))
            }
            _ => return Err(TranslateError::Order),
        };
        let column_kind = known_kind(&name, column_kind)?;
        let Some(kind) = column_kind.common(literal_kind) else {
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

    /// A call as one side of a comparison: a negative literal.
    fn call(call: &CallExpr) -> Result<Self, TranslateError> {
        let name = call.func_name.as_str();
        if name == operators::NEGATE
            && call.target.is_none()
            && let [arg] = call.args.as_slice()
            && let Expr::Literal(literal) = &arg.expr
        {
            return Self::negative(literal);
        }
        let shown = name.trim_matches('_').trim_start_matches('@');
        Err(TranslateError::Function(shown.to_owned()))
    }

    /// A literal as an operand.
    fn literal(literal: &LiteralValue) -> Result<Self, TranslateError> {
        Ok(match literal {
            LiteralValue::Null => Self::Null,
            LiteralValue::Boolean(v) => Self::Literal(Literal::Bool(*v.inner()), ValueKind::Bool),
            LiteralValue::Int(v) => Self::Literal(Literal::Int(*v.inner()), ValueKind::Int),
            LiteralValue::UInt(v) => Self::Literal(Literal::UInt(*v.inner()), ValueKind::Int),
            LiteralValue::Double(v) => Self::Literal(Literal::Float(*v.inner()), ValueKind::Float),
            LiteralValue::String(v) if v.inner().contains('\0') => {
                return Err(TranslateError::NulInString);
            }
            LiteralValue::String(v) => Self::Literal(Literal::String(v.inner().to_owned()), ValueKind::String),
            LiteralValue::Bytes(_) => return Err(TranslateError::Bytes),
        })
    }

    /// A negated number literal as an operand.
    fn negative(literal: &LiteralValue) -> Result<Self, TranslateError> {
        match literal {
            LiteralValue::Int(v) => {
                let negated = v.inner().checked_neg().ok_or(TranslateError::NegatedMinimum)?;
                Ok(Self::Literal(Literal::Int(negated), ValueKind::Int))
            }
            LiteralValue::Double(v) => Ok(Self::Literal(Literal::Float(-v.inner()), ValueKind::Float)),
            LiteralValue::Boolean(_)
            | LiteralValue::Bytes(_)
            | LiteralValue::Null
            | LiteralValue::String(_)
            | LiteralValue::UInt(_) => Err(TranslateError::NegatedNonNumber),
        }
    }
}

/// The kind of column `name`, an error if it is not known.
fn known_kind(name: &str, kind: Option<ValueKind>) -> Result<ValueKind, TranslateError> {
    kind.ok_or_else(|| TranslateError::UnknownColumnType(name.to_owned()))
}

/// The dotted name `expr` selects, like `name.en`.
fn qualified_name(expr: &IdedExpr) -> Option<String> {
    match &expr.expr {
        Expr::Ident(name) => Some(name.clone()),
        Expr::Select(select) if !select.test => Some(format!("{}.{}", qualified_name(&select.operand)?, select.field)),
        Expr::Unspecified
        | Expr::Call(_)
        | Expr::Comprehension(_)
        | Expr::List(_)
        | Expr::Literal(_)
        | Expr::Map(_)
        | Expr::Select(_)
        | Expr::Struct(_) => None,
    }
}

/// The value of a string literal.
fn string_literal(expr: &IdedExpr) -> Option<&str> {
    let Expr::Literal(LiteralValue::String(value)) = &expr.expr else {
        return None;
    };
    Some(value.inner())
}

impl CompareOp {
    /// The comparison CEL names `name`.
    fn cel(name: &str) -> Option<Self> {
        Some(match name {
            operators::EQUALS => Self::Eq,
            operators::NOT_EQUALS => Self::Ne,
            operators::LESS => Self::Lt,
            operators::LESS_EQUALS => Self::Le,
            operators::GREATER => Self::Gt,
            operators::GREATER_EQUALS => Self::Ge,
            _ => return None,
        })
    }
}

impl ValueKind {
    /// The type this and `other` compare as: numbers as floats once one is a float.
    fn common(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Float, Self::Int) | (Self::Int, Self::Float) => Some(Self::Float),
            (left, right) => (left == right).then_some(left),
        }
    }
}

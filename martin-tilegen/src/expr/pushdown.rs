//! Translates a filter expression into a [`Condition`] true for exactly the features it keeps.

use cel::common::ast::{CallExpr, Expr, IdedExpr, LiteralValue, operators};

use super::{CompiledExpr, FEATURE};
use crate::plan::{ColumnDef, CompareOp, Condition, Literal, Pattern, Polarity, Term, TranslateError, ValueKind};

impl CompiledExpr {
    /// The rows this filter keeps, reading only `columns`.
    pub(crate) fn condition(&self, columns: &[ColumnDef]) -> Result<Condition, TranslateError> {
        Translator { columns }.condition(&self.expr, Polarity::AsWritten)
    }
}

/// A part of an expression as a condition of `polarity`, reading only `columns`.
pub(crate) fn condition_of(
    expr: &IdedExpr,
    columns: &[ColumnDef],
    polarity: Polarity,
) -> Result<Condition, TranslateError> {
    Translator { columns }.condition(expr, polarity)
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
    /// A value read from the row.
    Row(RowValue),
}

/// A value read from the row.
enum RowValue {
    /// A column, with the kind of its values if it is known.
    /// It is `null` where the row has no value.
    Column(String, Option<ValueKind>),
    /// A function of a column, of `kind`.
    /// It is an error where the column is `null`.
    Function(Term, ValueKind),
}

/// A function of a string that a filter can push down.
enum StringFn {
    /// Matches the string against a pattern built from its literal argument.
    Match(fn(String) -> Pattern),
    /// Maps the string to a value of this kind.
    Map(fn(Box<Term>) -> Term, ValueKind),
}

impl StringFn {
    /// The function CEL calls `name`, if it is one, called on a target if `member`.
    fn named(name: &str, member: bool) -> Option<Self> {
        Some(match (name, member) {
            ("startsWith", true) => Self::Match(Pattern::Prefix),
            ("endsWith", true) => Self::Match(Pattern::Suffix),
            ("contains", true) => Self::Match(Pattern::Contains),
            ("size", _) => Self::Map(Term::Length, ValueKind::Int),
            ("lowerAscii", true) => Self::Map(Term::LowerAscii, ValueKind::String),
            ("upperAscii", true) => Self::Map(Term::UpperAscii, ValueKind::String),
            _ => return None,
        })
    }
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
            (name, Some(target), [arg]) if let Some(StringFn::Match(pattern)) = StringFn::named(name, true) => {
                let text = string_literal(arg).ok_or_else(|| TranslateError::NotALiteral(name.to_owned()))?;
                Ok(Condition::Like {
                    term: self.string_operand(name, target)?,
                    pattern: pattern(text.to_owned()),
                    polarity,
                })
            }
            _ => self.call_operand(call)?.into_condition(polarity),
        }
    }

    /// `value in list` of `polarity`, each element compared as it compares with `value`.
    fn in_list(&self, value: &IdedExpr, list: &IdedExpr, polarity: Polarity) -> Result<Condition, TranslateError> {
        let Operand::Row(row) = self.operand(value)? else {
            return Err(TranslateError::ConstantInList);
        };
        let nullable = row.nullable();
        let (term, kind) = row.typed()?;
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
                Operand::Row(_) => return Err(TranslateError::NotALiteralList),
            }
        }
        let negated = polarity.negated();
        let null_part = (nullable && has_null != negated).then(|| Condition::IsNull {
            term: term.clone(),
            polarity: Polarity::AsWritten,
        });
        let present = if lists.is_empty() {
            if negated {
                Condition::IsNull {
                    term,
                    polarity: Polarity::Negated,
                }
            } else {
                Condition::Const(false)
            }
        } else {
            polarity.any(lists.into_iter().map(|(kind, values)| Condition::In {
                term: term.clone(),
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
                self.call_operand(call)
            }
            Expr::Ident(_) => Err(TranslateError::Expression("`feature` as a value")),
            Expr::Comprehension(_) => Err(TranslateError::Expression("a comprehension")),
            Expr::List(_) => Err(TranslateError::Expression("a list")),
            Expr::Map(_) => Err(TranslateError::Expression("a map")),
            Expr::Struct(_) => Err(TranslateError::Expression("a message")),
            Expr::Unspecified => Err(TranslateError::Expression("an empty expression")),
        }
    }

    /// A call as one side of a comparison: a negative literal, or a string function of a column.
    fn call_operand(&self, call: &CallExpr) -> Result<Operand, TranslateError> {
        let name = call.func_name.as_str();
        let unsupported = || {
            let shown = name.trim_matches('_').trim_start_matches('@');
            TranslateError::Function(shown.to_owned())
        };
        let ((Some(arg), []) | (None, [arg])) = (call.target.as_deref(), call.args.as_slice()) else {
            return Err(unsupported());
        };
        if name == operators::NEGATE
            && let Expr::Literal(literal) = &arg.expr
        {
            return Operand::negative(literal);
        }
        let Some(StringFn::Map(wrap, kind)) = StringFn::named(name, call.target.is_some()) else {
            return Err(unsupported());
        };
        let term = self.string_operand(name, arg)?;
        Ok(Operand::Row(RowValue::Function(wrap(Box::new(term)), kind)))
    }

    /// The string column that `target` reads, for a call of `name`.
    fn string_operand(&self, name: &str, target: &IdedExpr) -> Result<Term, TranslateError> {
        let Operand::Row(row) = self.operand(target)? else {
            return Err(TranslateError::Constants);
        };
        match row.typed()? {
            (term, ValueKind::String) => Ok(term),
            _ => Err(TranslateError::NotAStringColumn(name.to_owned())),
        }
    }

    /// The column named `name`, with the kind of its values if it is known.
    fn column(&self, name: &str) -> Result<Operand, TranslateError> {
        let kind = self.column_kind(name)?;
        Ok(Operand::Row(RowValue::Column(name.to_owned(), kind)))
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
            Self::Row(row) => match row.typed()? {
                (term, ValueKind::Bool) => Ok(Condition::Compare {
                    left: term,
                    op: CompareOp::Eq,
                    right: Term::Literal(Literal::Bool(!polarity.negated())),
                    kind: ValueKind::Bool,
                }),
                (_, ValueKind::Int | ValueKind::Float | ValueKind::String) => Ok(Condition::Const(false)),
            },
            Self::Literal(..) | Self::Null => Ok(Condition::Const(false)),
        }
    }

    /// `self == right`, or `self != right` once negated.
    fn equals(self, right: Self, polarity: Polarity) -> Result<Condition, TranslateError> {
        match (self, right) {
            (Self::Row(row), other) | (other, Self::Row(row)) => row.equals(other, polarity),
            _ => Err(TranslateError::Constants),
        }
    }

    /// `self op right` for an ordering `op`.
    fn order(self, op: CompareOp, right: Self) -> Result<Condition, TranslateError> {
        let (row, op, literal, literal_kind) = match (self, right) {
            (Self::Row(row), Self::Literal(literal, kind)) => (row, op, literal, kind),
            (Self::Literal(literal, kind), Self::Row(row)) => (row, op.mirror(), literal, kind),
            _ => return Err(TranslateError::Order),
        };
        let (term, row_kind) = row.typed()?;
        let Some(kind) = row_kind.common(literal_kind) else {
            return Ok(Condition::Const(false));
        };
        let not_nan = (row_kind == ValueKind::Float).then(|| Condition::Compare {
            left: term.clone(),
            op: CompareOp::Ne,
            right: Term::Literal(Literal::Float(f64::NAN)),
            kind: ValueKind::Float,
        });
        let compare = Condition::Compare {
            left: term,
            op,
            right: Term::Literal(literal),
            kind,
        };
        Ok(Condition::and(std::iter::once(compare).chain(not_nan)))
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

impl RowValue {
    /// The term for this value, with the kind of its values.
    fn typed(self) -> Result<(Term, ValueKind), TranslateError> {
        match self {
            Self::Column(name, kind) => {
                let kind = known_kind(&name, kind)?;
                Ok((Term::Column(name), kind))
            }
            Self::Function(term, kind) => Ok((term, kind)),
        }
    }

    /// Whether this is `null`, not an error, where the row has no value.
    fn nullable(&self) -> bool {
        matches!(self, Self::Column(..))
    }

    /// `self == other`, or `self != other` once negated.
    fn equals(self, other: Operand, polarity: Polarity) -> Result<Condition, TranslateError> {
        match (self, other) {
            (Self::Column(name, _), Operand::Null) => Ok(Condition::IsNull {
                term: Term::Column(name),
                polarity,
            }),
            (Self::Column(name, kind), Operand::Literal(literal, literal_kind)) => {
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
            (Self::Column(name, kind), Operand::Row(Self::Column(other, other_kind))) => {
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
            (Self::Function(term, kind), Operand::Literal(literal, literal_kind))
                if let Some(kind) = kind.common(literal_kind) =>
            {
                Ok(Condition::Compare {
                    left: term,
                    op: polarity.op(CompareOp::Eq),
                    right: Term::Literal(literal),
                    kind,
                })
            }
            // A function never equals `null` or a value of another kind, and errs where its column is `null`.
            (Self::Function(term, _), Operand::Null | Operand::Literal(..)) => Ok(if polarity.negated() {
                Condition::IsNull {
                    term,
                    polarity: Polarity::Negated,
                }
            } else {
                Condition::Const(false)
            }),
            (Self::Column(..), Operand::Row(Self::Function(..))) | (Self::Function(..), Operand::Row(_)) => {
                Err(TranslateError::FunctionToColumn)
            }
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
    pub(crate) fn common(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Float, Self::Int) | (Self::Int, Self::Float) => Some(Self::Float),
            (left, right) => (left == right).then_some(left),
        }
    }
}

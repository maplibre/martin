//! Translates a filter expression into a [`Condition`] true for exactly the features it keeps.

use cel::common::ast::{CallExpr, Expr, IdedExpr, LiteralValue, operators};

use super::{CompiledExpr, FEATURE};
use crate::plan::{
    ColumnDef, CompareOp, Condition, Literal, Pattern, Term, TranslateError, ValueKind,
};

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
    /// A function of a column: an error where the column is `null`.
    Function,
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
            (operators::IN, None, [key, feature]) if is_feature(feature) => {
                let name = string_literal(key).ok_or(TranslateError::ComputedKey)?;
                Ok(Condition::IsNull {
                    term: self.column(name)?.term,
                    negated: !negated,
                })
            }
            (operators::IN, None, [value, list]) => self.in_list(value, list, negated),
            ("startsWith" | "endsWith" | "contains", Some(target), [arg]) => {
                let operand = self.operand(target)?;
                let text = string_literal(arg)
                    .ok_or_else(|| TranslateError::NotALiteral(name.to_owned()))?;
                match operand.kind {
                    Some(ValueKind::String) => {}
                    Some(_) => return Ok(Condition::Const(false)),
                    None => return Err(unknown_kind(&operand.term)),
                }
                let text = text.to_owned();
                let pattern = match name {
                    "startsWith" => Pattern::Prefix(text),
                    "endsWith" => Pattern::Suffix(text),
                    _ => Pattern::Contains(text),
                };
                Ok(Condition::Like {
                    term: operand.term,
                    pattern,
                    negated,
                })
            }
            _ => is_true(self.call_operand(call)?, negated),
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
            (Origin::Column | Origin::Function, Origin::Literal) => (left, op, right),
            (Origin::Literal, Origin::Column | Origin::Function) => (right, op.mirror(), left),
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

    /// `value in list`, negated if `negated`, each element compared as it compares with `value`.
    fn in_list(
        &self,
        value: &IdedExpr,
        list: &IdedExpr,
        negated: bool,
    ) -> Result<Condition, TranslateError> {
        let operand = self.operand(value)?;
        if !matches!(operand.origin, Origin::Column | Origin::Function) {
            return Err(TranslateError::ConstantInList);
        }
        if operand.kind.is_none() {
            return Err(unknown_kind(&operand.term));
        }
        let Expr::List(list) = &list.expr else {
            return Err(TranslateError::NotALiteralList);
        };
        let mut has_null = false;
        let mut lists: Vec<(ValueKind, Vec<Literal>)> = Vec::new();
        for element in &list.elements {
            let element = self.operand(element)?;
            match (element.origin, element.term) {
                (Origin::Null, _) => has_null = true,
                (Origin::Literal, Term::Literal(literal)) => {
                    let Some(kind) = common_kind(operand.kind, element.kind) else {
                        continue;
                    };
                    match lists.iter_mut().find(|(listed, _)| *listed == kind) {
                        Some((_, values)) => values.push(literal),
                        None => lists.push((kind, vec![literal])),
                    }
                }
                _ => return Err(TranslateError::NotALiteralList),
            }
        }
        let null_part =
            (operand.origin == Origin::Column && has_null != negated).then(|| Condition::IsNull {
                term: operand.term.clone(),
                negated: false,
            });
        let present = if lists.is_empty() {
            if negated {
                Condition::IsNull {
                    term: operand.term,
                    negated: true,
                }
            } else {
                Condition::Const(false)
            }
        } else {
            let parts = lists.into_iter().map(|(kind, values)| Condition::In {
                term: operand.term.clone(),
                values,
                kind,
                negated,
            });
            if negated {
                Condition::and(parts)
            } else {
                Condition::or(parts)
            }
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
            Expr::Literal(literal) => literal_operand(literal),
            Expr::Call(call) => {
                if call.func_name == operators::INDEX
                    && let [feature, key] = call.args.as_slice()
                    && is_feature(feature)
                {
                    let name = string_literal(key).ok_or(TranslateError::ComputedKey)?;
                    return self.column(name);
                }
                self.call_operand(call)
            }
            Expr::Unspecified
            | Expr::Ident(_)
            | Expr::Comprehension(_)
            | Expr::List(_)
            | Expr::Map(_)
            | Expr::Struct(_) => Err(TranslateError::Expression),
        }
    }

    /// A call as one side of a comparison: a negative literal, or a string function of a column.
    fn call_operand(&self, call: &CallExpr) -> Result<Operand, TranslateError> {
        let name = call.func_name.as_str();
        let unsupported = || {
            let shown = name.trim_matches('_').trim_start_matches('@');
            TranslateError::Function(shown.to_owned())
        };
        let ((Some(arg), []) | (None, [arg])) = (call.target.as_deref(), call.args.as_slice())
        else {
            return Err(unsupported());
        };
        if name == operators::NEGATE
            && let Expr::Literal(literal) = &arg.expr
        {
            return negative_literal(literal);
        }
        let (wrap, kind): (fn(Box<Term>) -> Term, _) = match name {
            "size" => (Term::Length, ValueKind::Int),
            "lowerAscii" => (Term::LowerAscii, ValueKind::String),
            "upperAscii" => (Term::UpperAscii, ValueKind::String),
            _ => return Err(unsupported()),
        };
        let inner = self.operand(arg)?;
        if inner.kind != Some(ValueKind::String)
            || !matches!(inner.origin, Origin::Column | Origin::Function)
        {
            return Err(TranslateError::NotAStringColumn(name.to_owned()));
        }
        Ok(Operand {
            term: wrap(Box::new(inner.term)),
            kind: Some(kind),
            origin: Origin::Function,
        })
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
        (Origin::Column | Origin::Function, Some(ValueKind::Bool)) => Ok(Condition::Compare {
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
        (Origin::Function, Origin::Null, _) | (Origin::Function, Origin::Literal, None) => {
            if equal {
                Condition::Const(false)
            } else {
                Condition::IsNull {
                    term: left.term,
                    negated: true,
                }
            }
        }
        (Origin::Function, Origin::Literal, Some(kind)) => Condition::Compare {
            left: left.term,
            op: if equal { CompareOp::Eq } else { CompareOp::Ne },
            right: right.term,
            kind,
        },
        (Origin::Column, Origin::Function, _)
        | (Origin::Function, Origin::Column | Origin::Function, _) => {
            return Err(TranslateError::FunctionToColumn);
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
        Term::Literal(_) | Term::Length(_) | Term::LowerAscii(_) | Term::UpperAscii(_) => {
            TranslateError::UnknownType
        }
    }
}

/// Whether `expr` is the `feature` variable.
fn is_feature(expr: &IdedExpr) -> bool {
    matches!(&expr.expr, Expr::Ident(name) if name == FEATURE)
}

/// The dotted name `expr` selects, like `name.en`.
fn qualified_name(expr: &IdedExpr) -> Option<String> {
    match &expr.expr {
        Expr::Ident(name) => Some(name.clone()),
        Expr::Select(select) if !select.test => Some(format!(
            "{}.{}",
            qualified_name(&select.operand)?,
            select.field
        )),
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

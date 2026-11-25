//! Writes a part of a compiled expression back as CEL, to name it in messages.

use std::fmt::{self, Display, Formatter};

use cel::common::ast::{Expr, IdedExpr, LiteralValue, operators};

use crate::expr::{FEATURE, feature_key};

/// `expr` as CEL source, with `feature['k']` written as `k` where `k` is an identifier.
pub(crate) struct Cel<'a>(pub(crate) &'a IdedExpr);

impl Display for Cel<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.0.expr {
            Expr::Unspecified => f.write_str("?"),
            Expr::Ident(name) => f.write_str(name),
            Expr::Literal(literal) => fmt_literal(literal, f),
            Expr::Select(select) if select.test => write!(f, "has({}.{})", Cel(&select.operand), select.field),
            Expr::Select(select) => write!(f, "{}.{}", Cel(&select.operand), select.field),
            Expr::List(list) => {
                f.write_str("[")?;
                list_of(&list.elements, f)?;
                f.write_str("]")
            }
            Expr::Comprehension(_) => f.write_str("(...)"),
            Expr::Map(_) | Expr::Struct(_) => f.write_str("{...}"),
            Expr::Call(call) => {
                if let Some(key) = feature_key(call) {
                    return if let Expr::Literal(LiteralValue::String(k)) = &key.expr
                        && is_identifier(k.inner())
                    {
                        f.write_str(k.inner())
                    } else {
                        write!(f, "{FEATURE}[{}]", Cel(key))
                    };
                }
                let args = call.args.as_slice();
                match (call.func_name.as_str(), call.target.as_deref(), args) {
                    (operators::INDEX, None, [target, key]) => write!(f, "{}[{}]", Operand(target), Cel(key)),
                    (operators::CONDITIONAL, None, [test, then, otherwise]) => {
                        write!(f, "{} ? {} : {}", Operand(test), Operand(then), Operand(otherwise))
                    }
                    (operators::LOGICAL_NOT, None, [arg]) => write!(f, "!{}", Operand(arg)),
                    (operators::NEGATE, None, [arg]) => write!(f, "-{}", Operand(arg)),
                    (operators::IN, None, [key, feature]) if matches!(&feature.expr, Expr::Ident(name) if name == FEATURE) =>
                    {
                        write!(f, "{} in {FEATURE}", Cel(key))
                    }
                    (name, None, [left, right]) if let Some(op) = infix(name) => {
                        write!(f, "{} {op} {}", Operand(left), Operand(right))
                    }
                    (name, Some(target), _) => {
                        write!(f, "{}.{name}(", Operand(target))?;
                        list_of(args, f)?;
                        f.write_str(")")
                    }
                    (name, None, _) => {
                        write!(f, "{name}(")?;
                        list_of(args, f)?;
                        f.write_str(")")
                    }
                }
            }
        }
    }
}

/// A sub-expression, in parentheses when it is an operator call.
struct Operand<'a>(&'a IdedExpr);

impl Display for Operand<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let Expr::Call(call) = &self.0.expr else {
            return Cel(self.0).fmt(f);
        };
        let name = call.func_name.as_str();
        if call.target.is_none() && (infix(name).is_some() || name == operators::CONDITIONAL) {
            write!(f, "({})", Cel(self.0))
        } else {
            Cel(self.0).fmt(f)
        }
    }
}

/// `items` as CEL, separated by commas.
fn list_of(items: &[IdedExpr], f: &mut Formatter<'_>) -> fmt::Result {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        Cel(item).fmt(f)?;
    }
    Ok(())
}

/// `literal` as CEL.
fn fmt_literal(literal: &LiteralValue, f: &mut Formatter<'_>) -> fmt::Result {
    match literal {
        LiteralValue::Null => f.write_str("null"),
        LiteralValue::Boolean(v) => write!(f, "{}", v.inner()),
        LiteralValue::Int(v) => write!(f, "{}", v.inner()),
        LiteralValue::UInt(v) => write!(f, "{}u", v.inner()),
        LiteralValue::Double(v) => write!(f, "{:?}", v.inner()),
        LiteralValue::String(v) => write!(f, "'{}'", v.inner().replace('\\', "\\\\").replace('\'', "\\'")),
        LiteralValue::Bytes(_) => f.write_str("b'...'"),
    }
}

/// The infix operator CEL writes the function `name` as.
fn infix(name: &str) -> Option<&'static str> {
    Some(match name {
        operators::LOGICAL_AND => "&&",
        operators::LOGICAL_OR => "||",
        operators::EQUALS => "==",
        operators::NOT_EQUALS => "!=",
        operators::LESS => "<",
        operators::LESS_EQUALS => "<=",
        operators::GREATER => ">",
        operators::GREATER_EQUALS => ">=",
        operators::ADD => "+",
        operators::SUBSTRACT => "-",
        operators::MULTIPLY => "*",
        operators::DIVIDE => "/",
        operators::MODULO => "%",
        operators::IN => "in",
        _ => return None,
    })
}

/// Whether `key` can be written without `feature[...]`.
fn is_identifier(key: &str) -> bool {
    let mut chars = key.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

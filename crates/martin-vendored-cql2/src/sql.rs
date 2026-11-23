use crate::{precedence, Error, Expr, Geometry};
use pg_escape::quote_identifier;
use sqlparser::ast::{
    Array as SqlArray, BinaryOperator, CastKind,
    DataType::{Date, Timestamp},
    Expr as SqlExpr,
    Expr::{Cast, Nested, Value as ValExpr},
    FunctionArgumentList, FunctionArguments, Ident, TimezoneInfo, Value,
};

/// Trait for converting expressions to SQLParser AST nodes.
pub trait ToSqlAst {
    /// Converts this expression to SQLParser AST.
    fn to_sql_ast(&self) -> Result<SqlExpr, Error>;
    /// Converts the expression to a SQL string.
    fn to_sql(&self) -> Result<String, Error>;
}

fn cast(arg: SqlExpr, data_type: sqlparser::ast::DataType) -> SqlExpr {
    Cast {
        expr: Box::new(arg),
        data_type,
        kind: CastKind::Cast,
        format: None,
    }
}

pub(crate) fn func(name: &str, args: Vec<SqlExpr>) -> Result<SqlExpr, Error> {
    Ok(SqlExpr::Function(sqlparser::ast::Function {
        name: sqlparser::ast::ObjectName(vec![sqlparser::ast::ObjectNamePart::Identifier(
            ident_inner(name)?,
        )]),
        args: FunctionArguments::List(FunctionArgumentList {
            duplicate_treatment: None,
            args: args
                .into_iter()
                .map(|arg| {
                    sqlparser::ast::FunctionArg::Unnamed(sqlparser::ast::FunctionArgExpr::Expr(arg))
                })
                .collect(),
            clauses: vec![],
        }),
        over: None,
        filter: None,
        null_treatment: None,
        within_group: vec![],
        uses_odbc_syntax: false,
        parameters: FunctionArguments::None,
    }))
}

/// A string literal.
///
/// A value holding neither a quote nor a backslash needs no escaping at all, and is written plainly.
/// Anything else is written as an escape string, `E'...'` - PostgreSQL syntax that DuckDB accepts
/// with the same meaning. That variant is printed by a dedicated escaper which rewrites every quote
/// and backslash unconditionally.
///
/// `SingleQuotedString` is deliberately not used for those values. Its printer exists to reproduce
/// SQL that sqlparser itself parsed, so it steps over a quote that already looks escaped - one
/// preceded by a backslash, or one of a `''` pair. That is right for a round trip and wrong for
/// data: `a''b` would be reprinted unchanged and read back as `a'b`, and a value ending in a
/// backslash would terminate the literal early.
fn lit_expr(value: &str) -> SqlExpr {
    let needs_escaping = value.contains('\'') || value.contains('\\');
    ValExpr(if needs_escaping {
        Value::EscapedStringLiteral(value.to_string()).into()
    } else {
        Value::SingleQuotedString(value.to_string()).into()
    })
}
/// A numeric literal.
///
/// An infinity or a NaN has no SQL spelling, and `f64::to_string` writes them as the bare words
/// `inf`, `-inf` and `NaN`, which a database reads as column references. `1 / 0` reduces to
/// `Float(inf)` and `0 / 0` to `Float(NaN)`, so both are reachable from an expression that parsed.
/// A numeric literal.
///
/// A non-finite value is written as a cast string rather than as a bare number: `inf` on its own is
/// a bare token that a database reads as a column reference. Both PostgreSQL and DuckDB accept
/// `'Infinity'`, `'-Infinity'` and `'NaN'` cast to a floating-point type, and compare them as the
/// IEEE values they name - which is also how `..` interval bounds are already rendered.
fn float_expr(value: &f64) -> SqlExpr {
    if value.is_finite() {
        return ValExpr(Value::Number(value.to_string(), false).into());
    }
    let name = if value.is_nan() {
        "NaN"
    } else if value.is_sign_positive() {
        "Infinity"
    } else {
        "-Infinity"
    };
    cast(
        lit_expr(name),
        sqlparser::ast::DataType::Double(sqlparser::ast::ExactNumberInfo::None),
    )
}

fn args2ast(args: &[Box<Expr>]) -> Result<Vec<SqlExpr>, Error> {
    args.iter()
        .map(|arg| arg.to_sql_ast())
        .collect::<Result<Vec<_>, _>>()
}

/// What an operator's SQL rendering requires of its operand count.
enum Arity {
    /// The rendering indexes exactly this many operands.
    Exactly(usize),
    /// The rendering chains its operands pairwise and accepts any number from this floor up.
    AtLeast(usize),
    /// The rendering is a function call, which takes its arguments as a list.
    Any,
}

/// The operand count an operator's SQL rendering requires.
///
/// The arithmetic operators are n-ary, as they are in cql2-text: `{"op": "+", "args": [a, b, c]}`
/// renders as `a + b + c` in both. They chain to the left, so `a - b - c` is `(a - b) - c` and
/// `a / b / c` is `(a / b) / c`, which is how SQL and cql2-text alike read the flat form. Two
/// operands is the floor, because one would print as that operand by itself with the operator
/// silently dropped.
///
/// `^` is not one of them. It renders as `power(a, b)`, which engines define as taking a base and an
/// exponent, and `to_text` also requires exactly two. `div` is a function call in both encodings, so
/// it takes its arguments as a list like any other.
fn sql_arity(op: &str) -> Arity {
    match op {
        "isNull" | "not" => Arity::Exactly(1),
        "between" => Arity::Exactly(3),
        "in" | "like" | "=" | "a_equals" | "<>" | ">" | ">=" | "<" | "<=" | "^" | "a_contains"
        | "a_containedBy" | "a_overlaps" => Arity::Exactly(2),
        "+" | "-" | "*" | "/" | "%" => Arity::AtLeast(2),
        _ => Arity::Any,
    }
}

/// The operand requirement for an operator *as SQL renders it*.
///
/// Almost every operator keeps its CQL2 requirement, because cql2-text and SQL share PostgreSQL's
/// precedence shape. Exactly one operator differs: `^` is infix in cql2-text but renders as
/// `power(a, b)` here, whose arguments the call's own parentheses and commas already delimit. Every
/// other operator that renders as a SQL function call is written as a function call in cql2-text
/// too, so it already carries `ATOM` precedence and demands nothing of its operands.
///
/// The opposite case is *not* handled here: `a_contains`, `a_containedBy`, `a_overlaps` and
/// `a_equals` are function calls in cql2-text - ATOM, requiring nothing of their operands - but
/// render as the infix `@>`, `<@`, `@@` and, for `a_equals`, a conjunction of two `@>`. Their
/// operands are therefore never parenthesized, which is sound only because what an array predicate
/// takes as an operand (an array, a property, a function call) is itself an atom. The `a_equals`
/// rendering wraps itself in parentheses for the same reason the `t_*` renderings do: it is a
/// conjunction where the caller expects an atom.
fn sql_operands(op: &str) -> precedence::Operands {
    match op {
        // `a ^ b` renders as `power(a, b)`.
        "^" => precedence::Operands { first: 0, rest: 0 },
        _ => precedence::operands(op),
    }
}

/// Converts operands, parenthesizing any that bind more loosely than the operator they hang off of.
///
/// A SQL AST records no grouping of its own: `Display` renders `BinaryOp` as `{left} {op} {right}`,
/// never emitting parentheses, so a nested `a AND (b OR c)` would print as `a AND b OR c` and
/// reparse as `(a AND b) OR c`. `Nested` is sqlparser's "parentheses were written here" node, and is
/// what its own parser produces for `(...)`.
fn args2ast_grouped(op: &str, args: &[Box<Expr>]) -> Result<Vec<SqlExpr>, Error> {
    let requirement = sql_operands(op);
    args.iter()
        .enumerate()
        .map(|(index, arg)| {
            let ast = arg.to_sql_ast()?;
            Ok(if requirement.needs_parens(index, arg) {
                wrap(ast)
            } else {
                ast
            })
        })
        .collect::<Result<Vec<_>, _>>()
}
/// The character a `LIKE` pattern escapes its wildcards with.
///
/// The evaluator's matcher and PostgreSQL both read `\` this way; DuckDB reads a pattern with no
/// `ESCAPE` clause literally, wildcards and all. Stating it makes the pattern mean one thing
/// everywhere, so `like(textfield, 'item\_1')` selects the same rows in each.
const LIKE_ESCAPE: char = '\\';

/// A binary comparison over the two operands `sql_arity` has already checked for.
fn binop(op: BinaryOperator, args: Vec<SqlExpr>) -> SqlExpr {
    let [left, right] = args
        .try_into()
        .expect("sql_arity checked the operand count");
    cmp(left, op, right)
}

/// Array equality, as the *set* comparison the evaluator makes it.
///
/// `array_op` compares two arrays as sets: it collects each into a `HashSet`, so neither the order
/// of the elements nor a repeat of one changes the answer. SQL's `=` on arrays is positional -
/// `[1,2,3] = [3,2,1]` is false - so rendering `a_equals` as `=` would give the two backends
/// different answers for the same expression.
///
/// Mutual containment says exactly what set equality says, and is spelled the same way in both
/// dialects: `@>` and its DuckDB rewrite `list_has_all` are already how `a_contains` renders. Sorting
/// the two arrays would be the other way to say it, but PostgreSQL has no portable array sort.
fn set_equality(args: Vec<SqlExpr>) -> SqlExpr {
    let [left, right] = args
        .try_into()
        .expect("sql_arity checked the operand count");
    wrap(andop(vec![
        cmp(left.clone(), BinaryOperator::AtArrow, right.clone()),
        cmp(right, BinaryOperator::AtArrow, left),
    ]))
}

struct Targs {
    left_start: SqlExpr,
    left_end: SqlExpr,
    right_start: SqlExpr,
    right_end: SqlExpr,
}

/// The SQL spelling of an interval bound.
///
/// `unbounded` is the value `".."` stands for at this end of the range; databases spell an
/// unbounded timestamp `infinity`.
fn lit_or_prop_to_ts(arg: &Expr, unbounded: &str) -> Result<SqlExpr, Error> {
    Ok(match arg {
        Expr::Property { property } => ident(property)?,
        Expr::Literal(v) => cast(
            lit_expr(if v == ".." { unbounded } else { v }),
            Timestamp(None, TimezoneInfo::WithTimeZone),
        ),
        _ => return Err(Error::OperationError()),
    })
}

fn lit_or_prop_to_date(arg: &Expr) -> Result<SqlExpr, Error> {
    Ok(match arg {
        Expr::Property { property } => ident(property)?,
        Expr::Literal(v) => cast(lit_expr(v), Date),
        _ => return Err(Error::OperationError()),
    })
}

/// Checks an interval carries exactly the two bounds the callers index.
fn interval_bounds(interval: &[Box<Expr>]) -> Result<(&Expr, &Expr), Error> {
    match interval {
        [start, end] => Ok((start, end)),
        _ => Err(Error::InvalidNumberOfArguments {
            name: "interval".to_string(),
            actual: interval.len(),
            expected: 2,
        }),
    }
}

/// The two endpoints of an interval, each as a SQL timestamp.
///
/// Which end is open decides what `".."` means there, so the two sentinels are paired here rather
/// than at each call site.
fn interval_endpoints(interval: &[Box<Expr>]) -> Result<(SqlExpr, SqlExpr), Error> {
    let (lo, hi) = interval_bounds(interval)?;
    Ok((
        lit_or_prop_to_ts(lo, "-infinity")?,
        lit_or_prop_to_ts(hi, "infinity")?,
    ))
}

/// A timestamp rendered as a SQL literal.
fn timestamp_literal(ts: jiff::Timestamp) -> SqlExpr {
    cast(
        lit_expr(&ts.to_string()),
        Timestamp(None, TimezoneInfo::WithTimeZone),
    )
}

fn t_arg_to_interval(arg: &Expr) -> Result<(SqlExpr, SqlExpr), Error> {
    match arg {
        Expr::Interval { interval } => interval_endpoints(interval),
        Expr::Property { property } => {
            let start = ident(property)?;
            Ok((start.clone(), start))
        }
        // A date names a day, not an instant: `DateRange::try_from` widens it to
        // `[T00:00:00, T23:59:59.999999999]`, and the SQL rendering has to say the same thing.
        Expr::Date { date } => {
            let day = crate::temporal::DateRange::try_from(Expr::Date { date: date.clone() })?;
            Ok((timestamp_literal(day.start), timestamp_literal(day.end)))
        }
        Expr::Timestamp { timestamp } => {
            let start = lit_or_prop_to_ts(timestamp, "infinity")?;
            Ok((start.clone(), start))
        }
        _ => Err(Error::OperationError()),
    }
}

fn t_args(args: &[Box<Expr>]) -> Result<Targs, Error> {
    let [left, right] = args else {
        return Err(Error::InvalidNumberOfArguments {
            name: "temporal predicate".to_string(),
            actual: args.len(),
            expected: 2,
        });
    };
    let (left_start, left_end) = t_arg_to_interval(left)?;
    let (right_start, right_end) = t_arg_to_interval(right)?;
    Ok(Targs {
        left_start,
        left_end,
        right_start,
        right_end,
    })
}

/// The SQL for an Allen interval relation.
///
/// Every one of these is a comparison between the endpoints of the two operands, so they all start
/// by resolving each operand to its `[start, end]` pair. `op` is a canonical CQL2 name; the caller
/// dispatches here on [`crate::expr::TEMPORALOPS`], which is exactly the set covered below.
fn temporal_sql(op: &str, args: &[Box<Expr>]) -> Result<SqlExpr, Error> {
    let t = t_args(args)?;
    Ok(match op {
        "t_before" => ltop(t.left_end, t.right_start),
        "t_after" => ltop(t.right_end, t.left_start),
        "t_meets" => eqop(t.left_end, t.right_start),
        "t_metBy" => eqop(t.right_end, t.left_start),
        // `overlaps`: the earlier range begins first, the two share an interior,
        // and the earlier one ends first.
        "t_overlaps" => wrap(andop(vec![
            ltop(t.left_start, t.right_start.clone()),
            ltop(t.right_start, t.left_end.clone()),
            ltop(t.left_end, t.right_end),
        ])),
        "t_overlappedBy" => wrap(andop(vec![
            ltop(t.right_start, t.left_start.clone()),
            ltop(t.left_start, t.right_end.clone()),
            ltop(t.right_end, t.left_end),
        ])),
        "t_starts" => wrap(andop(vec![
            eqop(t.left_start, t.right_start.clone()),
            ltop(t.left_end, t.right_end),
        ])),
        "t_startedBy" => wrap(andop(vec![
            eqop(t.right_start, t.left_start.clone()),
            ltop(t.right_end, t.left_end),
        ])),
        "t_during" => wrap(andop(vec![
            gtop(t.left_start, t.right_start),
            ltop(t.left_end, t.right_end),
        ])),
        "t_contains" => wrap(andop(vec![
            gtop(t.right_start, t.left_start),
            ltop(t.right_end, t.left_end),
        ])),
        "t_finishes" => wrap(andop(vec![
            eqop(t.left_end, t.right_end),
            gtop(t.left_start, t.right_start),
        ])),
        "t_finishedBy" => wrap(andop(vec![
            eqop(t.right_end, t.left_end),
            gtop(t.right_start, t.left_start),
        ])),
        "t_equals" => wrap(andop(vec![
            eqop(t.left_start, t.right_start),
            eqop(t.left_end, t.right_end),
        ])),
        // Wrapped outside the `NOT` as well, so the whole predicate is self-delimiting like every
        // other `t_*` rendering.
        "t_disjoint" => wrap(notop(wrap(andop(vec![
            lteop(t.left_start, t.right_end),
            gteop(t.left_end, t.right_start),
        ])))),
        "t_intersects" => wrap(andop(vec![
            lteop(t.left_start, t.right_end),
            gteop(t.left_end, t.right_start),
        ])),
        _ => return Err(Error::InvalidOperator(op.to_string())),
    })
}

/// Chains operands with an associative connective. An empty chain has no rendering.
fn chainop(op: BinaryOperator, args: Vec<SqlExpr>) -> Result<SqlExpr, Error> {
    let name = op.to_string().to_lowercase();
    args.into_iter()
        .reduce(|left, right| SqlExpr::BinaryOp {
            left: Box::new(left),
            op: op.clone(),
            right: Box::new(right),
        })
        .ok_or(Error::InvalidNumberOfArguments {
            name,
            actual: 0,
            expected: 1,
        })
}

/// The `t_*` renderings build their own conjunctions, always with operands to chain.
fn andop(args: Vec<SqlExpr>) -> SqlExpr {
    chainop(BinaryOperator::And, args).expect("callers supply at least one operand")
}

/// A binary comparison between two already-rendered operands.
fn cmp(left: SqlExpr, op: BinaryOperator, right: SqlExpr) -> SqlExpr {
    SqlExpr::BinaryOp {
        left: Box::new(left),
        op,
        right: Box::new(right),
    }
}

fn ltop(left: SqlExpr, right: SqlExpr) -> SqlExpr {
    cmp(left, BinaryOperator::Lt, right)
}

fn gtop(left: SqlExpr, right: SqlExpr) -> SqlExpr {
    cmp(left, BinaryOperator::Gt, right)
}

fn lteop(left: SqlExpr, right: SqlExpr) -> SqlExpr {
    cmp(left, BinaryOperator::LtEq, right)
}

fn gteop(left: SqlExpr, right: SqlExpr) -> SqlExpr {
    cmp(left, BinaryOperator::GtEq, right)
}

fn eqop(left: SqlExpr, right: SqlExpr) -> SqlExpr {
    cmp(left, BinaryOperator::Eq, right)
}

fn notop(arg: SqlExpr) -> SqlExpr {
    SqlExpr::UnaryOp {
        op: sqlparser::ast::UnaryOperator::Not,
        expr: Box::new(arg),
    }
}

fn wrap(arg: SqlExpr) -> SqlExpr {
    Nested(Box::new(arg))
}

/// A name rendered as a SQL identifier.
///
/// An empty name is rejected: it has no SQL spelling, and printing it emits nothing at all, so
/// `Expr::Property { property: String::new() }` compared against `1` would render as the fragment
/// ` = 1`. Neither encoding forbids an empty name - `{"property": ""}` is accepted by the schema,
/// and `""` is a quoted identifier the cql2-text grammar takes - so this is reachable input rather
/// than a defensive check.
fn ident_inner(property: &str) -> Result<Ident, Error> {
    if property.is_empty() {
        return Err(Error::EmptySqlIdentifier);
    }
    let p = quote_identifier(property);
    Ok(if p.starts_with('"') && p.ends_with('"') {
        Ident::with_quote('"', p[1..p.len() - 1].to_string())
    } else {
        Ident::new(p)
    })
}

fn ident(property: &str) -> Result<SqlExpr, Error> {
    Ok(SqlExpr::Identifier(ident_inner(property)?))
}

impl ToSqlAst for Expr {
    /// Converts this expression to SQLParser AST.
    fn to_sql_ast(&self) -> Result<SqlExpr, Error> {
        Ok(match self {
            Expr::Bool(v) => ValExpr(Value::Boolean(*v).into()),
            Expr::Float(v) => float_expr(v),
            Expr::Literal(v) => lit_expr(v),
            Expr::Date { ref date } => lit_or_prop_to_date(date.as_ref())?,
            // An instant has no distinguishable open end, so which sentinel `".."` stands for is
            // arbitrary here: `TIMESTAMP('..')` renders as `CAST('infinity' AS TIMESTAMP WITH TIME
            // ZONE)`, and `-infinity` would have been just as good.
            Expr::Timestamp { ref timestamp } => lit_or_prop_to_ts(timestamp.as_ref(), "infinity")?,
            Expr::Interval { ref interval } => {
                let (start, end) = interval_endpoints(interval)?;
                SqlExpr::Array(SqlArray {
                    elem: vec![start, end],
                    named: true,
                })
            }
            Expr::Null => ValExpr(Value::Null.into()),
            Expr::Geometry(v) => match v {
                Geometry::GeoJSON(v) => {
                    let s = lit_expr(&v.to_string());
                    func("st_geomfromgeojson", vec![s])?
                }
                Geometry::Wkt(v) => {
                    let s = lit_expr(&v.to_string());
                    func("st_geomfromtext", vec![s])?
                }
            },

            Expr::BBox { bbox } => func("st_makeenvelope", args2ast(bbox)?)?,
            Expr::Array(ref v) => SqlExpr::Array(SqlArray {
                elem: args2ast(v)?,
                named: true,
            }),
            Expr::Property { property } => ident(property)?,
            Expr::Operation { op, args } => {
                // Route through the canonical spelling, so the schema's capitalization and the
                // operator aliases apply here exactly as they do to a parsed expression. A name CQL2
                // does not define comes back unchanged, keeping the author's case for the function
                // fallback below.
                let canonical = crate::expr::canonical_op(op);
                let op_str = canonical.as_str();
                let a = args2ast_grouped(op_str, args)?;
                // Checked before the arms index into `a`, so a malformed expression is an error
                // rather than a panic. Operators rendered as function calls take any arity.
                let required = match sql_arity(op_str) {
                    Arity::Exactly(expected) if a.len() != expected => Some(expected),
                    Arity::AtLeast(minimum) if a.len() < minimum => Some(minimum),
                    _ => None,
                };
                if let Some(expected) = required {
                    return Err(Error::InvalidNumberOfArguments {
                        name: op_str.to_string(),
                        actual: a.len(),
                        expected,
                    });
                }
                match op_str {
                    "isNull" => SqlExpr::IsNull(Box::new(a[0].clone())),
                    "not" => notop(a[0].clone()),
                    "between" => SqlExpr::Between {
                        expr: Box::new(a[0].clone()),
                        negated: false,
                        low: Box::new(a[1].clone()),
                        high: Box::new(a[2].clone()),
                    },
                    "in" => {
                        let expr = a[0].clone();
                        let items = a[1].clone();
                        SqlExpr::AnyOp {
                            left: Box::new(expr),
                            compare_op: BinaryOperator::Eq,
                            right: Box::new(items),
                            is_some: true,
                        }
                    }
                    "like" => {
                        let expr = a[0].clone();
                        let pattern = a[1].clone();
                        SqlExpr::Like {
                            expr: Box::new(expr),
                            pattern: Box::new(pattern),
                            // The escape character is stated rather than left to the engine, so
                            // every engine reads the pattern the way the evaluator does. `\` is
                            // what the evaluator escapes with and what PostgreSQL defaults to;
                            // DuckDB has no default at all, so without this `'item\_1'` matches
                            // `item_1` here and a disjoint set of rows there.
                            escape_char: Some(Box::new(ValExpr(
                                Value::SingleQuotedString(LIKE_ESCAPE.to_string()).into(),
                            ))),
                            negated: false,
                            any: false,
                        }
                    }
                    "accenti" => func("strip_accents", a)?,
                    "casei" => func("lower", a)?,
                    "and" => chainop(BinaryOperator::And, a)?,
                    "or" => chainop(BinaryOperator::Or, a)?,
                    "=" => binop(BinaryOperator::Eq, a),
                    "a_equals" => set_equality(a),
                    "<>" => binop(BinaryOperator::NotEq, a),
                    ">" => binop(BinaryOperator::Gt, a),
                    ">=" => binop(BinaryOperator::GtEq, a),
                    "<" => binop(BinaryOperator::Lt, a),
                    "<=" => binop(BinaryOperator::LtEq, a),
                    "+" => chainop(BinaryOperator::Plus, a)?,
                    "-" => chainop(BinaryOperator::Minus, a)?,
                    "*" => chainop(BinaryOperator::Multiply, a)?,
                    "/" => chainop(BinaryOperator::Divide, a)?,
                    "%" => chainop(BinaryOperator::Modulo, a)?,
                    "^" => func("power", a)?,
                    "s_intersects" => func("st_intersects", a)?,
                    "s_equals" => func("st_equals", a)?,
                    "s_within" => func("st_within", a)?,
                    "s_contains" => func("st_contains", a)?,
                    "s_crosses" => func("st_crosses", a)?,
                    "s_overlaps" => func("st_overlaps", a)?,
                    "s_touches" => func("st_touches", a)?,
                    "s_disjoint" => func("st_disjoint", a)?,
                    "a_contains" => binop(BinaryOperator::AtArrow, a),
                    "a_containedBy" => binop(BinaryOperator::ArrowAt, a),
                    "a_overlaps" => binop(BinaryOperator::AtAt, a),
                    name if crate::expr::TEMPORALOPS.contains(&name) => temporal_sql(name, args)?,
                    _ => func(&canonical, a)?,
                }
            }
        })
    }

    /// Converts the expression to a SQL string.
    fn to_sql(&self) -> Result<String, Error> {
        Ok(self.to_sql_ast()?.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::ToSqlAst;
    use crate::Expr;

    #[test]
    fn test_basic_expression() {
        let expr: Expr = "1 + 2 > 4".parse().unwrap();
        let sql_ast = expr.to_sql_ast().unwrap();
        let sql_str = sql_ast.to_string();
        assert_eq!(sql_str, "1 + 2 > 4");
    }

    #[test]
    fn test_t_before_expression() {
        // t_before([start1, end1], [start2, end2]) => end1 < start2.
        // A date is the whole day, so the bound is its first instant, matching how the evaluator
        // reads it.
        let expr: Expr = "t_before(ts_start, DATE('2020-02-01'))".parse().unwrap();
        let sql_ast = expr.to_sql_ast().expect("to_sql_ast failed");
        let sql_str = sql_ast.to_string();
        assert_eq!(
            sql_str,
            "ts_start < CAST('2020-02-01T00:00:00Z' AS TIMESTAMP WITH TIME ZONE)"
        );
    }

    #[test]
    fn test_bbox() {
        let expr: Expr = "bbox(1, 2, 3, 4)".parse().unwrap();
        assert_eq!(expr.to_sql().unwrap(), "st_makeenvelope(1, 2, 3, 4)");
    }

    /// An empty name prints as nothing, so `"" = 1` would render as the fragment ` = 1`.
    #[test]
    fn empty_property_name_is_rejected() {
        let expr = Expr::Operation {
            op: "=".to_string(),
            args: vec![
                Box::new(Expr::Property {
                    property: String::new(),
                }),
                Box::new(Expr::Float(1.0)),
            ],
        };
        assert!(matches!(
            expr.to_sql(),
            Err(crate::Error::EmptySqlIdentifier)
        ));
    }

    /// `inf` and `NaN` render as the values they name, not as bare tokens.
    ///
    /// Emitted bare, `inf` is an identifier a database reads as a column reference.
    #[test]
    fn non_finite_numbers_render_as_cast_literals() {
        for (value, name) in [
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (f64::NAN, "NaN"),
        ] {
            let sql = Expr::Float(value).to_sql().expect("renders as SQL");
            assert_eq!(sql, format!("CAST('{name}' AS DOUBLE)"));
        }
        // Reachable from an expression that parsed: division by zero reduces to an infinity.
        let divided: Expr = "1 / 0".parse().unwrap();
        assert_eq!(
            divided.reduce(None).unwrap().to_sql().expect("renders"),
            "CAST('Infinity' AS DOUBLE)"
        );
    }
}

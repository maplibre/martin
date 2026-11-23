//! Renders the rows a plan's layers take as a SQL condition on the table they come from.

use std::fmt::{self, Display, Formatter};

use martin_tilegen::plan::{CompareOp, Condition, GeometryType, Literal, Pattern, Term, ValueKind};
use postgres_protocol::escape::{escape_identifier, escape_literal};

use super::query_tables::{PropertyColumn, linear_geometry, property_column};
use crate::config::file::postgres::TableInfo;

const UPPER: &str = "'ABCDEFGHIJKLMNOPQRSTUVWXYZ'";
const LOWER: &str = "'abcdefghijklmnopqrstuvwxyz'";

/// `condition` over the columns of `info`, each read as the scan selects it.
pub fn condition_sql(info: &TableInfo, condition: &Condition) -> String {
    Sql(info, condition).to_string()
}

/// A plan node as SQL over the columns of `info`.
struct Sql<'a, T>(&'a TableInfo, &'a T);

/// A literal as SQL.
/// A float is quoted so that `NaN` and `inf` parse, and its kind casts it to `float8`.
struct LiteralSql<'a>(&'a Literal);

/// A value compared as `kind`: numbers compared as floats are cast.
struct AsKind<T>(T, ValueKind);

impl Display for Sql<'_, Condition> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let Self(info, condition) = *self;
        match condition {
            Condition::Const(value) => LiteralSql(&Literal::Bool(*value)).fmt(f),
            Condition::And(parts) => join(f, info, parts, " AND "),
            Condition::Or(parts) => join(f, info, parts, " OR "),
            Condition::Compare { left, op, right, kind } => {
                let ordered = !matches!(op, CompareOp::Eq | CompareOp::Ne);
                write!(
                    f,
                    "{lhs}{collate} {op} {rhs}",
                    lhs = AsKind(Sql(info, left), *kind),
                    collate = collate_sql(info, left, right, *kind, ordered),
                    op = op.sql(),
                    rhs = AsKind(Sql(info, right), *kind)
                )
            }
            Condition::Distinct {
                left,
                right,
                kind,
                polarity,
            } => write!(
                f,
                "{lhs}{collate} IS {not}DISTINCT FROM {rhs}",
                lhs = AsKind(Sql(info, left), *kind),
                collate = collate_sql(info, left, right, *kind, false),
                not = polarity.sql(),
                rhs = AsKind(Sql(info, right), *kind)
            ),
            Condition::IsNull { term, polarity } => {
                write!(f, "{term} IS {not}NULL", term = Sql(info, term), not = polarity.sql())
            }
            Condition::In {
                term,
                values,
                kind,
                polarity,
            } => {
                let values = fmt::from_fn(|f| {
                    for (i, value) in values.iter().enumerate() {
                        if i > 0 {
                            f.write_str(", ")?;
                        }
                        AsKind(LiteralSql(value), *kind).fmt(f)?;
                    }
                    Ok(())
                });
                write!(
                    f,
                    "{term}{collate} {not}IN ({values})",
                    term = AsKind(Sql(info, term), *kind),
                    collate = if *kind == ValueKind::String {
                        code_point_sql(info, term)
                    } else {
                        ""
                    },
                    not = polarity.sql()
                )
            }
            Condition::Like {
                term,
                pattern,
                polarity,
            } => {
                let escape = |text: &str| text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
                let pattern = match pattern {
                    Pattern::Prefix(text) => format!("{}%", escape(text)),
                    Pattern::Suffix(text) => format!("%{}", escape(text)),
                    Pattern::Contains(text) => format!("%{}%", escape(text)),
                };
                write!(
                    f,
                    "{term}{collate} {not}LIKE {pattern}",
                    term = Sql(info, term),
                    collate = code_point_sql(info, term),
                    not = polarity.sql(),
                    pattern = escape_literal(&pattern)
                )
            }
            Condition::Geometry(geometry) => {
                let types = match geometry {
                    GeometryType::Point => "'ST_Point', 'ST_MultiPoint'",
                    GeometryType::Line => "'ST_LineString', 'ST_MultiLineString'",
                    GeometryType::Polygon => "'ST_Polygon', 'ST_MultiPolygon', 'ST_Triangle'",
                };
                write!(
                    f,
                    "ST_GeometryType({geometry}) IN ({types})",
                    geometry = linear_geometry(info)
                )
            }
        }
    }
}

impl Display for Sql<'_, Term> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let Self(info, term) = *self;
        match term {
            Term::Literal(literal) => LiteralSql(literal).fmt(f),
            Term::Column(name) => match property_column(info, name) {
                PropertyColumn::Typed(column) => f.write_str(&escape_identifier(column)),
                PropertyColumn::Text(column) => write!(f, "{}::text", escape_identifier(column)),
            },
            Term::Length(inner) => write!(f, "char_length({})", Sql(info, &**inner)),
            Term::LowerAscii(inner) => {
                write!(f, "translate({}, {UPPER}, {LOWER})", Sql(info, &**inner))
            }
            Term::UpperAscii(inner) => {
                write!(f, "translate({}, {LOWER}, {UPPER})", Sql(info, &**inner))
            }
        }
    }
}

impl Display for LiteralSql<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.0 {
            Literal::Bool(value) => f.write_str(if *value { "TRUE" } else { "FALSE" }),
            Literal::String(value) => f.write_str(&escape_literal(value)),
            Literal::Float(value) => write!(f, "'{value}'"),
            Literal::Int(value) => write!(f, "{value}"),
            Literal::UInt(value) => write!(f, "{value}"),
        }
    }
}

impl<T: Display> Display for AsKind<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let Self(sql, kind) = self;
        if *kind == ValueKind::Float {
            write!(f, "({sql})::float8")
        } else {
            sql.fmt(f)
        }
    }
}

/// `parts` joined by `separator`, each nested `AND` or `OR` in parentheses.
fn join(f: &mut Formatter<'_>, info: &TableInfo, parts: &[Condition], separator: &str) -> fmt::Result {
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            f.write_str(separator)?;
        }
        match part {
            Condition::And(_) | Condition::Or(_) => write!(f, "({})", Sql(info, part))?,
            Condition::Const(_)
            | Condition::Compare { .. }
            | Condition::Distinct { .. }
            | Condition::IsNull { .. }
            | Condition::In { .. }
            | Condition::Like { .. }
            | Condition::Geometry(_) => Sql(info, part).fmt(f)?,
        }
    }
    Ok(())
}

/// ` COLLATE "C"` where comparing `left` with `right` as `kind` would not compare strings by code point.
fn collate_sql(info: &TableInfo, left: &Term, right: &Term, kind: ValueKind, ordered: bool) -> &'static str {
    let columns = matches!((left, right), (Term::Column(_), Term::Column(_)));
    if kind == ValueKind::String && (ordered || columns || loose(info, left) || loose(info, right)) {
        " COLLATE \"C\""
    } else {
        ""
    }
}

/// ` COLLATE "C"` where `term` would not compare by code point.
fn code_point_sql(info: &TableInfo, term: &Term) -> &'static str {
    if loose(info, term) { " COLLATE \"C\"" } else { "" }
}

/// Whether `term` is a string under a nondeterministic collation, which compares strings loosely.
fn loose(info: &TableInfo, term: &Term) -> bool {
    match term {
        Term::Column(name) => info.has_nondeterministic_collation(name),
        Term::LowerAscii(inner) | Term::UpperAscii(inner) => loose(info, inner),
        Term::Literal(_) | Term::Length(_) => false,
    }
}

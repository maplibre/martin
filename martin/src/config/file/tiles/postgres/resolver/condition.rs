//! Renders the rows a plan's layers take as a SQL condition on the table they come from.

use martin_tilegen::plan::{CompareOp, Condition, GeometryType, Literal, Term, ValueKind};
use postgres_protocol::escape::{escape_identifier, escape_literal};

use super::query_tables::{PropertyColumn, linear_geometry, property_column};
use crate::config::file::postgres::TableInfo;

/// `condition` over the columns of `info`, each read as the scan selects it.
pub fn condition_sql(info: &TableInfo, condition: &Condition) -> String {
    match condition {
        Condition::Const(value) => bool_sql(*value).to_owned(),
        Condition::And(parts) => join(info, parts, " AND "),
        Condition::Or(parts) => join(info, parts, " OR "),
        Condition::Compare {
            left,
            op,
            right,
            kind,
        } => {
            let ordered = !matches!(op, CompareOp::Eq | CompareOp::Ne);
            format!(
                "{lhs}{collate} {op} {rhs}",
                lhs = value_sql(info, left, *kind),
                collate = collate_sql(info, left, right, *kind, ordered),
                op = op.sql(),
                rhs = value_sql(info, right, *kind)
            )
        }
        Condition::Distinct {
            left,
            right,
            kind,
            negated,
        } => format!(
            "{lhs}{collate} IS {not}DISTINCT FROM {rhs}",
            lhs = value_sql(info, left, *kind),
            collate = collate_sql(info, left, right, *kind, false),
            not = not_sql(*negated),
            rhs = value_sql(info, right, *kind)
        ),
        Condition::IsNull { term, negated } => format!(
            "{term} IS {not}NULL",
            term = term_sql(info, term),
            not = not_sql(*negated)
        ),
        Condition::Geometry(geometry) => {
            let types = match geometry {
                GeometryType::Point => "'ST_Point', 'ST_MultiPoint'",
                GeometryType::Line => "'ST_LineString', 'ST_MultiLineString'",
                GeometryType::Polygon => "'ST_Polygon', 'ST_MultiPolygon', 'ST_Triangle'",
            };
            format!(
                "ST_GeometryType({geometry}) IN ({types})",
                geometry = linear_geometry(info)
            )
        }
    }
}

/// `parts` joined by `separator`, each nested `AND` or `OR` in parentheses.
fn join(info: &TableInfo, parts: &[Condition], separator: &str) -> String {
    let parts: Vec<String> = parts
        .iter()
        .map(|part| match part {
            Condition::And(_) | Condition::Or(_) => format!("({})", condition_sql(info, part)),
            Condition::Const(_)
            | Condition::Compare { .. }
            | Condition::Distinct { .. }
            | Condition::IsNull { .. }
            | Condition::Geometry(_) => condition_sql(info, part),
        })
        .collect();
    parts.join(separator)
}

/// A term as a value of `kind`.
fn value_sql(info: &TableInfo, term: &Term, kind: ValueKind) -> String {
    as_kind(term_sql(info, term), kind)
}

/// `sql` as a value of `kind`: numbers compared as floats are cast.
fn as_kind(sql: String, kind: ValueKind) -> String {
    if kind == ValueKind::Float {
        format!("({sql})::float8")
    } else {
        sql
    }
}

/// A term as the scan reads it.
fn term_sql(info: &TableInfo, term: &Term) -> String {
    match term {
        Term::Literal(literal) => literal_sql(literal),
        Term::Column(name) => column_sql(info, name),
    }
}

/// ` COLLATE "C"` where comparing `left` with `right` as `kind` would not compare strings by code point.
fn collate_sql(
    info: &TableInfo,
    left: &Term,
    right: &Term,
    kind: ValueKind,
    ordered: bool,
) -> &'static str {
    let loose = |term: &Term| matches!(term, Term::Column(name) if info.has_nondeterministic_collation(name));
    let columns = matches!((left, right), (Term::Column(_), Term::Column(_)));
    if kind == ValueKind::String && (ordered || columns || loose(left) || loose(right)) {
        " COLLATE \"C\""
    } else {
        ""
    }
}

/// A column as the scan selects it.
fn column_sql(info: &TableInfo, name: &str) -> String {
    match property_column(info, name) {
        PropertyColumn::Typed(column) => escape_identifier(column),
        PropertyColumn::Text(column) => format!("{}::text", escape_identifier(column)),
    }
}

/// A literal as SQL.
/// A float is quoted so that `NaN` and `inf` parse, and its kind casts it to `float8`.
fn literal_sql(literal: &Literal) -> String {
    match literal {
        Literal::Bool(value) => bool_sql(*value).to_owned(),
        Literal::String(value) => escape_literal(value),
        Literal::Float(value) => format!("'{value}'"),
        Literal::Int(value) => value.to_string(),
        Literal::UInt(value) => value.to_string(),
    }
}

/// A boolean as SQL.
fn bool_sql(value: bool) -> &'static str {
    if value { "TRUE" } else { "FALSE" }
}

/// `NOT ` when `negated`.
fn not_sql(negated: bool) -> &'static str {
    if negated { "NOT " } else { "" }
}

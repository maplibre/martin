//! Renders the rows a plan's layers take as a SQL condition on the table they come from.

use martin_tilegen::plan::{CompareOp, Condition, GeometryType, Literal, Term, ValueKind};
use postgres_protocol::escape::{escape_identifier, escape_literal};

use super::query_tables::{linear_geometry, property_column};
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
            let collate =
                if *kind == ValueKind::String && !matches!(op, CompareOp::Eq | CompareOp::Ne) {
                    " COLLATE \"C\""
                } else {
                    ""
                };
            format!(
                "{}{collate} {} {}",
                term_sql(info, left, *kind),
                op_sql(*op),
                term_sql(info, right, *kind)
            )
        }
        Condition::Distinct {
            left,
            right,
            kind,
            negated,
        } => format!(
            "{} IS {}DISTINCT FROM {}",
            term_sql(info, left, *kind),
            not_sql(*negated),
            term_sql(info, right, *kind)
        ),
        Condition::IsNull { term, negated } => format!(
            "{} IS {}NULL",
            term_sql(info, term, ValueKind::String),
            not_sql(*negated)
        ),
        Condition::Geometry(geometry) => {
            let types = match geometry {
                GeometryType::Point => "'ST_Point', 'ST_MultiPoint'",
                GeometryType::Line => "'ST_LineString', 'ST_MultiLineString'",
                GeometryType::Polygon => "'ST_Polygon', 'ST_MultiPolygon', 'ST_Triangle'",
            };
            format!("ST_GeometryType({}) IN ({types})", linear_geometry(info))
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

/// A term as a value of `kind`: numbers compared as floats are cast.
fn term_sql(info: &TableInfo, term: &Term, kind: ValueKind) -> String {
    let sql = match term {
        Term::Literal(literal) => return literal_sql(literal, kind),
        Term::Column(name) => column_sql(info, name),
    };
    if kind == ValueKind::Float {
        format!("({sql})::float8")
    } else {
        sql
    }
}

/// A column as the scan selects it.
fn column_sql(info: &TableInfo, name: &str) -> String {
    let label = info
        .properties
        .as_ref()
        .and_then(|properties| properties.get(name))
        .map_or("", String::as_str);
    let (column, cast) = property_column(info, name, label);
    let column = escape_identifier(column);
    if cast {
        format!("{column}::text")
    } else {
        column
    }
}

/// A literal as a value of `kind`.
fn literal_sql(literal: &Literal, kind: ValueKind) -> String {
    let sql = match literal {
        Literal::Bool(value) => return bool_sql(*value).to_owned(),
        Literal::String(value) => return escape_literal(value),
        Literal::Float(value) => return format!("'{value}'::float8"),
        Literal::Int(value) if *value < 0 => format!("({value})"),
        Literal::Int(value) => value.to_string(),
        Literal::UInt(value) => value.to_string(),
    };
    if kind == ValueKind::Float {
        format!("{sql}::float8")
    } else {
        sql
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

/// A comparison operator as SQL.
fn op_sql(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Eq => "=",
        CompareOp::Ne => "<>",
        CompareOp::Lt => "<",
        CompareOp::Le => "<=",
        CompareOp::Gt => ">",
        CompareOp::Ge => ">=",
    }
}

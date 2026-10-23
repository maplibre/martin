//! What the `PostgreSQL` catalog says about the values of a table's columns: `NOT NULL`, the labels of an enum, and
//! the values a `CHECK` on one column lists.

use martin_core::tiles::postgres::{PooledConnection, PostgresError};
use martin_tilegen::plan::{ColumnFacts, Literal};
use postgres_protocol::escape::escape_identifier;

use crate::config::file::postgres::TableInfo;

/// The columns of the table `info` reads, each with its name in the table, whether it is never `null`, the labels of
/// its enum type, and the definitions of the valid `CHECK`s on it alone.
const COLUMNS_SQL: &str = "
SELECT a.attname::text,
       quote_ident(a.attname),
       a.attnotnull AND NOT EXISTS (
         SELECT 1 FROM pg_constraint n
         WHERE n.conrelid = a.attrelid AND n.contype = 'n' AND n.conkey = ARRAY[a.attnum] AND NOT n.convalidated
       ),
       CASE WHEN t.typtype = 'e' THEN ARRAY(
         SELECT e.enumlabel::text FROM pg_enum e WHERE e.enumtypid = t.oid ORDER BY e.enumsortorder
       ) END,
       ARRAY(
         SELECT pg_get_constraintdef(c.oid) FROM pg_constraint c
         WHERE c.conrelid = a.attrelid AND c.contype = 'c' AND c.conkey = ARRAY[a.attnum] AND c.convalidated
       )
FROM pg_attribute a JOIN pg_type t ON t.oid = a.atttypid
WHERE a.attrelid = $1::text::regclass AND a.attnum > 0 AND NOT a.attisdropped";

/// What the catalog says about the properties of the table `info`, read on `conn`.
///
/// # Errors
///
/// If the catalog cannot be read.
pub async fn table_facts(conn: &PooledConnection, info: &TableInfo) -> Result<Vec<ColumnFacts>, PostgresError> {
    let table = format!("{}.{}", escape_identifier(&info.schema), escape_identifier(&info.table));
    let rows = conn
        .query(COLUMNS_SQL, &[&table])
        .await
        .map_err(|e| PostgresError::PostgresError(e, "reading the constraints of a table"))?;
    let mut out = Vec::new();
    for property in info.properties.iter().flat_map(|p| p.keys()) {
        let column = info.table_column(property);
        let Some(row) = rows.iter().find(|row| row.get::<_, &str>(0) == column) else {
            continue;
        };
        let quoted: &str = row.get(1);
        let not_null: bool = row.get(2);
        let labels: Option<Vec<String>> = row.get(3);
        let checks: Vec<String> = row.get(4);
        let mut values = labels.map(|labels| labels.into_iter().map(Literal::String).collect::<Vec<_>>());
        for allowed in checks.iter().filter_map(|check| check_values(check, quoted)) {
            values = Some(match values {
                Some(values) => values.into_iter().filter(|v| allowed.contains(v)).collect(),
                None => allowed,
            });
        }
        if not_null || values.is_some() {
            out.push(ColumnFacts {
                column: property.clone(),
                not_null,
                values,
            });
        }
    }
    Ok(out)
}

/// The values `CHECK` definition `check` allows column `quoted`, when it compares the column to one value or to an
/// `ARRAY` of them, as `pg_get_constraintdef` prints `column = value` and `column IN (values)`.
fn check_values(check: &str, quoted: &str) -> Option<Vec<Literal>> {
    let body = unwrap_parens(check.strip_prefix("CHECK ")?);
    let (left, right) = body.split_once(" = ")?;
    let left = unwrap_parens(left);
    let column = match left.rsplit_once("::") {
        Some((column, _)) => unwrap_parens(column),
        None => left,
    };
    if column != quoted {
        return None;
    }
    let Some(any) = right.strip_prefix("ANY ") else {
        return Some(vec![literal(right)?]);
    };
    let array = unwrap_parens(any);
    let array = match array.rsplit_once("::") {
        Some((array, cast)) if cast.ends_with("[]") => unwrap_parens(array),
        _ => array,
    };
    let items = array.strip_prefix("ARRAY[")?.strip_suffix(']')?;
    split_items(items).into_iter().map(literal).collect()
}

/// `text` without the parentheses around all of it.
fn unwrap_parens(mut text: &str) -> &str {
    while let Some(inner) = text.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
        if !balanced(inner) {
            break;
        }
        text = inner;
    }
    text
}

/// Whether every parenthesis outside a quoted string of `text` closes one opened before it.
fn balanced(text: &str) -> bool {
    let mut depth = 0_usize;
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '\'' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => match depth.checked_sub(1) {
                Some(d) => depth = d,
                None => return false,
            },
            _ => {}
        }
    }
    depth == 0
}

/// The items of a comma-separated list, splitting only outside quoted strings.
fn split_items(items: &str) -> Vec<&str> {
    let mut quoted = false;
    let comma = |c: char| {
        quoted ^= c == '\'';
        c == ',' && !quoted
    };
    items.split(comma).map(str::trim).collect()
}

/// A constant as `PostgreSQL` prints it, e.g. `'a'::text`, `'-1'::integer`, `(2)::numeric` or `2`, read as the type
/// of its cast.
fn literal(text: &str) -> Option<Literal> {
    let text = unwrap_parens(text);
    let (value, cast) = match text.strip_prefix('\'') {
        Some(rest) => {
            let (value, cast) = rest.rsplit_once('\'')?;
            (Printed::Quoted(value.replace("''", "'")), cast.strip_prefix("::"))
        }
        None => match text.rsplit_once("::") {
            Some((value, cast)) => (Printed::Bare(unwrap_parens(value)), Some(cast)),
            None => (Printed::Bare(text), None),
        },
    };
    let text = match &value {
        Printed::Quoted(text) => text.as_str(),
        Printed::Bare(text) => text,
    };
    match cast {
        Some("integer" | "bigint" | "smallint") => text.parse().ok().map(Literal::Int),
        Some("numeric" | "double precision" | "real") => text.parse().ok().map(Literal::Float),
        Some("boolean") => text.parse().ok().map(Literal::Bool),
        Some(_) => Some(Literal::String(text.to_owned())),
        None => match value {
            Printed::Quoted(text) => Some(Literal::String(text)),
            Printed::Bare(text) => (text.parse().ok().map(Literal::Bool))
                .or_else(|| text.parse().ok().map(Literal::Int))
                .or_else(|| text.parse().ok().map(Literal::Float)),
        },
    }
}

/// The text of a constant as `PostgreSQL` prints it.
enum Printed<'a> {
    /// A quoted string, its quotes undoubled.
    Quoted(String),
    /// A number or a boolean.
    Bare(&'a str),
}

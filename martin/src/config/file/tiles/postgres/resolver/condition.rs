//! Renders the rows a plan's layers take as a SQL condition on the table they come from.

use martin_tilegen::plan::{Condition, GeometryType};

use super::query_tables::linear_geometry;
use crate::config::file::postgres::TableInfo;

/// `condition` over the columns of `info`, each read as the scan selects it.
pub fn condition_sql(info: &TableInfo, condition: &Condition) -> String {
    match condition {
        Condition::Const(value) => bool_sql(*value).to_owned(),
        Condition::And(parts) => join(info, parts, " AND "),
        Condition::Or(parts) => join(info, parts, " OR "),
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
            Condition::Const(_) | Condition::Geometry(_) => condition_sql(info, part),
        })
        .collect();
    parts.join(separator)
}

/// A boolean as SQL.
fn bool_sql(value: bool) -> &'static str {
    if value { "TRUE" } else { "FALSE" }
}

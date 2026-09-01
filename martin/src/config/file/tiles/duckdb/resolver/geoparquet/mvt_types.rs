/// The `DuckDB` type a property column must be cast to before `ST_AsMVT` will accept it.
///
/// `ST_AsMVT` only accepts `VARCHAR`, `FLOAT`, `DOUBLE`, `INTEGER`, `BIGINT` and `BOOLEAN`.
/// Every other scalar type is mapped to the closest of those; container and binary types
/// have no faithful MVT representation and yield [`None`] so the caller can drop them.
#[must_use]
pub(crate) fn mvt_property_type(duckdb_type: &str) -> Option<&'static str> {
    let normalized = duckdb_type.trim().to_ascii_uppercase();

    if normalized.ends_with(']')
        || normalized.starts_with("STRUCT(")
        || normalized.starts_with("MAP(")
        || normalized.starts_with("UNION(")
    {
        return None;
    }
    if normalized.starts_with("DECIMAL(") {
        return Some("DOUBLE");
    }
    if normalized.starts_with("ENUM(") {
        return Some("VARCHAR");
    }

    match normalized.as_str() {
        "FLOAT" => Some("FLOAT"),
        "DOUBLE" => Some("DOUBLE"),
        "BOOLEAN" => Some("BOOLEAN"),
        "INTEGER" | "TINYINT" | "SMALLINT" | "UTINYINT" | "USMALLINT" => Some("INTEGER"),
        "BIGINT" | "UINTEGER" => Some("BIGINT"),
        "VARCHAR"
        | "UBIGINT"
        | "HUGEINT"
        | "UHUGEINT"
        | "BIGNUM"
        | "DATE"
        | "TIME"
        | "TIME WITH TIME ZONE"
        | "TIMESTAMP"
        | "TIMESTAMP WITH TIME ZONE"
        | "TIMESTAMP_S"
        | "TIMESTAMP_MS"
        | "TIMESTAMP_NS"
        | "INTERVAL"
        | "UUID"
        | "BIT"
        | "JSON" => Some("VARCHAR"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::varchar("VARCHAR", Some("VARCHAR"))]
    #[case::float("FLOAT", Some("FLOAT"))]
    #[case::double("DOUBLE", Some("DOUBLE"))]
    #[case::integer("INTEGER", Some("INTEGER"))]
    #[case::bigint("BIGINT", Some("BIGINT"))]
    #[case::boolean("BOOLEAN", Some("BOOLEAN"))]
    #[case::tinyint("TINYINT", Some("INTEGER"))]
    #[case::smallint("SMALLINT", Some("INTEGER"))]
    #[case::usmallint("USMALLINT", Some("INTEGER"))]
    #[case::uinteger("UINTEGER", Some("BIGINT"))]
    #[case::ubigint("UBIGINT", Some("VARCHAR"))]
    #[case::hugeint("HUGEINT", Some("VARCHAR"))]
    #[case::bignum("BIGNUM", Some("VARCHAR"))]
    #[case::decimal("DECIMAL(18,3)", Some("DOUBLE"))]
    #[case::date("DATE", Some("VARCHAR"))]
    #[case::timestamptz("TIMESTAMP WITH TIME ZONE", Some("VARCHAR"))]
    #[case::timestamp_ns("TIMESTAMP_NS", Some("VARCHAR"))]
    #[case::interval("INTERVAL", Some("VARCHAR"))]
    #[case::uuid("UUID", Some("VARCHAR"))]
    #[case::json("JSON", Some("VARCHAR"))]
    #[case::enum_type("ENUM('a', 'b')", Some("VARCHAR"))]
    #[case::lowercase("varchar", Some("VARCHAR"))]
    #[case::blob("BLOB", None)]
    #[case::geometry("GEOMETRY", None)]
    #[case::list("VARCHAR[]", None)]
    #[case::array("INTEGER[3]", None)]
    #[case::struct_type("STRUCT(a VARCHAR)", None)]
    #[case::struct_list("STRUCT(freeform VARCHAR, locality VARCHAR)[]", None)]
    #[case::map("MAP(VARCHAR, VARCHAR)", None)]
    #[case::union("UNION(a INTEGER, b VARCHAR)", None)]
    #[case::unknown("SOMETHING_ELSE", None)]
    fn mvt_property_type_cases(#[case] input: &str, #[case] expected: Option<&str>) {
        assert_eq!(mvt_property_type(input), expected);
    }
}

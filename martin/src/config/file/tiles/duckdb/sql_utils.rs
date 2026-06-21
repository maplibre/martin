/// Double-quote a DuckDB identifier and escape embedded quotes.
#[must_use]
pub fn escape_identifier(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Single-quote an SQL string literal and escape embedded apostrophes.
#[must_use]
pub fn escape_sql_string(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Format an EPSG SRID as a DuckDB CRS string literal (e.g. `'EPSG:4326'`).
#[must_use]
pub fn epsg_crs(srid: i32) -> String {
    escape_sql_string(&format!("EPSG:{srid}"))
}

#[cfg(test)]
#[cfg(feature = "unstable-duckdb")]
mod tests {
    use super::{epsg_crs, escape_identifier, escape_sql_string};

    #[test]
    fn escapes_identifier_with_quotes() {
        assert_eq!(escape_identifier("roads"), "\"roads\"");
        assert_eq!(escape_identifier("my\"table"), "\"my\"\"table\"");
    }

    #[test]
    fn escapes_sql_string_with_apostrophe() {
        assert_eq!(escape_sql_string("simple"), "'simple'");
        assert_eq!(escape_sql_string("O'Brien"), "'O''Brien'");
    }

    #[test]
    fn formats_epsg_crs() {
        assert_eq!(epsg_crs(4326), "'EPSG:4326'");
        assert_eq!(epsg_crs(3857), "'EPSG:3857'");
    }
}

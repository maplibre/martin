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

/// Build a `read_parquet(...)` table-source expression for inline SQL `FROM` clauses.
#[must_use]
pub fn read_parquet_from_expr(path_or_url: &str) -> String {
    format!("read_parquet({})", escape_sql_string(path_or_url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::simple("roads", "\"roads\"")]
    #[case::embedded_quote("my\"table", "\"my\"\"table\"")]
    #[case::empty("", "\"\"")]
    #[case::quote_only("\"", "\"\"\"\"")]
    #[case::multiple_quotes("a\"\"b", "\"a\"\"\"\"b\"")]
    #[case::dot_in_identifier("schema.table", "\"schema.table\"")]
    #[case::unicode("Straße", "\"Straße\"")]
    fn escape_identifier_cases(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(escape_identifier(input), expected);
    }

    #[rstest]
    #[case::simple("simple", "'simple'")]
    #[case::embedded_apostrophe("O'Brien", "'O''Brien'")]
    #[case::empty("", "''")]
    #[case::apostrophe_only("'", "''''")]
    #[case::multiple_apostrophes("a''b", "'a''''b'")]
    #[case::double_quotes_preserved("\"quoted\"", "'\"quoted\"'")]
    #[case::unicode("Straße", "'Straße'")]
    fn escape_sql_string_cases(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(escape_sql_string(input), expected);
    }

    #[rstest]
    #[case::wgs84(4326, "'EPSG:4326'")]
    #[case::web_mercator(3857, "'EPSG:3857'")]
    #[case::zero(0, "'EPSG:0'")]
    #[case::negative(-1, "'EPSG:-1'")]
    fn epsg_crs_cases(#[case] srid: i32, #[case] expected: &str) {
        assert_eq!(epsg_crs(srid), expected);
    }

    #[rstest]
    #[case::local_path("/data/buildings.parquet", "read_parquet('/data/buildings.parquet')")]
    #[case::remote_url(
        "https://example.org/data.parquet",
        "read_parquet('https://example.org/data.parquet')"
    )]
    #[case::embedded_apostrophe(
        "/data/O'Brien.parquet",
        "read_parquet('/data/O''Brien.parquet')"
    )]
    fn read_parquet_from_expr_cases(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(read_parquet_from_expr(input), expected);
    }
}

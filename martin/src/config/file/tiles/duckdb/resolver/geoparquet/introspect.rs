use std::collections::BTreeMap;
use std::num::NonZeroI32;

use martin_core::tiles::duckdb::DuckDBPool;

use crate::config::file::tiles::duckdb::resolver::errors::{GeoparquetError, GeoparquetResult};
use crate::config::file::tiles::duckdb::sources::GeoParquetEntry;
use crate::config::file::tiles::duckdb::sql_utils::{escape_identifier, escape_sql_string};

/// Column metadata discovered from a GeoParquet file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeoParquetIntrospection {
    pub geometry_column: String,
    pub srid: NonZeroI32,
    pub property_columns: BTreeMap<String, String>,
}

pub(crate) fn geoparquet_from_expr(entry: &GeoParquetEntry) -> GeoparquetResult<(String, String)> {
    let path_or_url = entry
        .geoparquet
        .to_str()
        .ok_or_else(|| GeoparquetError::NonUtf8Path(entry.geoparquet.clone()))?;
    Ok((
        format!("read_parquet({})", escape_sql_string(path_or_url)),
        path_or_url.to_string(),
    ))
}

pub(crate) async fn introspect(
    pool: &DuckDBPool,
    from_expr: &str,
    source_label: &str,
    entry: &GeoParquetEntry,
) -> GeoparquetResult<GeoParquetIntrospection> {
    let all_columns = query_columns(pool, from_expr, source_label).await?;
    let geometry_columns = all_columns
        .iter()
        .filter(|(_, column_type)| column_type.to_ascii_uppercase().contains("GEOMETRY"))
        .map(|(name, column_type)| (name.clone(), column_type.clone()))
        .collect::<Vec<_>>();
    let geometry_column = select_geometry_column(entry, &geometry_columns, &all_columns)?;

    if let Some(id_column) = &entry.id_column {
        if !all_columns.contains_key(id_column) {
            return Err(GeoparquetError::IdColumnNotFound(id_column.clone()));
        }
    }

    let property_columns = all_columns
        .iter()
        .filter(|(name, _)| {
            name.as_str() != geometry_column.as_str()
                && entry.id_column.as_deref() != Some(name.as_str())
        })
        .map(|(name, column_type)| (name.clone(), column_type.clone()))
        .collect();

    let srid = match entry.srid {
        Some(srid) => NonZeroI32::new(srid).ok_or_else(|| {
            GeoparquetError::SridNonPositive(
                geometry_column.clone(),
                "(configuration)".to_string(),
                srid,
            )
        })?,
        None => query_srid(pool, from_expr, source_label, &geometry_column).await?,
    };

    Ok(GeoParquetIntrospection {
        geometry_column,
        srid,
        property_columns,
    })
}

fn select_geometry_column(
    entry: &GeoParquetEntry,
    geometry_columns: &[(String, String)],
    all_columns: &BTreeMap<String, String>,
) -> GeoparquetResult<String> {
    if let Some(requested) = &entry.geometry_column {
        if geometry_columns.iter().any(|(name, _)| name == requested) {
            return Ok(requested.clone());
        }
        if let Some(column_type) = all_columns.get(requested) {
            return Err(GeoparquetError::NotGeometryColumn(
                requested.clone(),
                column_type.clone(),
            ));
        }
        return Err(GeoparquetError::GeometryColumnNotFound(requested.clone()));
    }

    match geometry_columns.len() {
        0 => Err(GeoparquetError::NoGeometryColumn),
        1 => Ok(geometry_columns[0].0.clone()),
        _ => Err(GeoparquetError::AmbiguousGeometryColumn(
            geometry_columns
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
        )),
    }
}

async fn query_columns(
    pool: &DuckDBPool,
    from_expr: &str,
    source_label: &str,
) -> GeoparquetResult<BTreeMap<String, String>> {
    let query = format!("DESCRIBE SELECT * FROM {from_expr}");
    let query_for_error = query.clone();
    let source_label = source_label.to_string();

    pool.generate_tile(move |conn| {
        Ok(conn.prepare(&query).and_then(|mut stmt| {
            stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
        }))
    })
    .await?
    .map_err(|source| {
        GeoparquetError::introspection_query(source, source_label, "columns", query_for_error)
    })
    .map(|rows| rows.into_iter().collect())
}

async fn query_srid(
    pool: &DuckDBPool,
    from_expr: &str,
    source_label: &str,
    geometry_column: &str,
) -> GeoparquetResult<NonZeroI32> {
    let escaped_geometry_column = escape_identifier(geometry_column);
    let query = format!(
        "SELECT ST_CRS({escaped_geometry_column}) \
         FROM {from_expr} \
         WHERE {escaped_geometry_column} IS NOT NULL \
         LIMIT 1"
    );
    let query_for_error = query.clone();
    let source_label = source_label.to_string();
    let geometry_column = geometry_column.to_string();

    let crs = pool
        .generate_tile(move |conn| {
            use duckdb::OptionalExt as _;

            Ok(conn
                .query_row(&query, [], |row| row.get::<_, Option<String>>(0))
                .optional())
        })
        .await?
        .map_err(|source| {
            GeoparquetError::introspection_query(source, source_label, "srid", query_for_error)
        })?;

    match crs.flatten() {
        None => Err(GeoparquetError::SridUnknown(geometry_column)),
        Some(crs) => parse_crs_to_srid(&crs, &geometry_column),
    }
}

pub(crate) fn parse_crs_to_srid(crs: &str, geometry_column: &str) -> GeoparquetResult<NonZeroI32> {
    let geometry_column = geometry_column.to_string();
    let crs = crs.trim();
    if crs.is_empty() {
        return Err(GeoparquetError::SridEmpty(geometry_column, crs.to_string()));
    }

    if crs.eq_ignore_ascii_case("OGC:CRS84") {
        return Ok(NonZeroI32::new(4326).expect("4326 is non-zero"));
    }

    let Some(auth_code) = crs
        .strip_prefix("EPSG:")
        .or_else(|| crs.strip_prefix("epsg:"))
    else {
        return Err(GeoparquetError::SridUnsupportedCrs(
            geometry_column,
            crs.to_string(),
        ));
    };

    let srid = auth_code.parse::<i32>().map_err(|_| {
        GeoparquetError::SridInvalidEpsgCode(geometry_column.clone(), crs.to_string())
    })?;

    NonZeroI32::new(srid).ok_or(GeoparquetError::SridNonPositive(
        geometry_column,
        crs.to_string(),
        srid,
    ))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::epsg_upper("EPSG:4326", 4326)]
    #[case::epsg_lower("epsg:3857", 3857)]
    #[case::ogc_crs84("OGC:CRS84", 4326)]
    fn parse_crs_to_srid_accepts_epsg_and_crs84(#[case] crs: &str, #[case] expected: i32) {
        assert_eq!(
            parse_crs_to_srid(crs, "geom").expect("crs parsed").get(),
            expected
        );
    }

    #[test]
    fn parse_crs_to_srid_rejects_unknown_crs() {
        let err = parse_crs_to_srid("UNKNOWN:1", "geom").expect_err("unknown crs");
        assert!(matches!(err, GeoparquetError::SridUnsupportedCrs(..)));
    }
}

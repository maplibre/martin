use std::collections::BTreeMap;
use std::num::NonZeroI32;

use martin_core::tiles::duckdb::DuckDBPool;
use tracing::warn;

use crate::config::file::tiles::duckdb::resolver::errors::{DuckDbSourceError, DuckDbSourceResult};
use crate::config::file::tiles::duckdb::resolver::geoparquet::covering::CoveringBbox;
use crate::config::file::tiles::duckdb::resolver::mvt_types::mvt_property_type;
use crate::config::file::tiles::duckdb::sources::MvtLayerOptions;
use crate::config::file::tiles::duckdb::sql_utils::escape_identifier;

/// Column metadata discovered from a relation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerIntrospection {
    pub geometry_column: String,
    pub srid: NonZeroI32,
    /// Column name to the `DuckDB` type it must be cast to for `ST_AsMVT`, not the type it
    /// has on disk. Columns with no MVT representation are excluded.
    pub property_columns: BTreeMap<String, String>,
    /// The file's `GeoParquet` 1.1 `covering` bounding box, when it declares one.
    pub covering: Option<CoveringBbox>,
}

pub(crate) async fn introspect(
    pool: &DuckDBPool,
    from_expr: &str,
    source_label: &str,
    layer: &MvtLayerOptions,
) -> DuckDbSourceResult<LayerIntrospection> {
    let all_columns = query_columns(pool, from_expr, source_label).await?;
    let geometry_columns = all_columns
        .iter()
        .filter(|(_, column_type)| column_type.to_ascii_uppercase().contains("GEOMETRY"))
        .map(|(name, column_type)| (name.clone(), column_type.clone()))
        .collect::<Vec<_>>();
    let geometry_column = select_geometry_column(layer, &geometry_columns, &all_columns)?;

    if let Some(id_column) = &layer.id_column
        && !all_columns.contains_key(id_column)
    {
        return Err(DuckDbSourceError::IdColumnNotFound(id_column.clone()));
    }

    let property_columns = select_property_columns(
        &all_columns,
        &geometry_column,
        layer.id_column.as_deref(),
        source_label,
    );

    let srid = match layer.srid {
        Some(srid) => NonZeroI32::new(srid).ok_or_else(|| {
            DuckDbSourceError::SridNonPositive(
                geometry_column.clone(),
                "(configuration)".to_owned(),
                srid,
            )
        })?,
        None => query_srid(pool, from_expr, source_label, &geometry_column).await?,
    };

    Ok(LayerIntrospection {
        geometry_column,
        srid,
        property_columns,
        covering: None,
    })
}

fn select_property_columns(
    all_columns: &BTreeMap<String, String>,
    geometry_column: &str,
    id_column: Option<&str>,
    source_label: &str,
) -> BTreeMap<String, String> {
    let mut properties = BTreeMap::new();
    let mut dropped = Vec::new();

    for (name, column_type) in all_columns {
        if name == geometry_column || id_column == Some(name.as_str()) {
            continue;
        }
        match mvt_property_type(column_type) {
            Some(mvt_type) => {
                properties.insert(name.clone(), mvt_type.to_owned());
            }
            None => dropped.push(format!("{name} ({column_type})")),
        }
    }

    match dropped.as_slice() {
        [] => {}
        [col] => {
            warn!(
                "Ignoring {col} column of {source_label} with no MVT representation. \
             Vector tiles can only carry text, numeric and boolean properties."
            );
        }
        cols => {
            warn!(
                "Ignoring {} columns of {source_label} with no MVT representation: {}. \
             Vector tiles can only carry text, numeric and boolean properties.",
                cols.len(),
                cols.join(", ")
            );
        }
    }

    properties
}

fn select_geometry_column(
    layer: &MvtLayerOptions,
    geometry_columns: &[(String, String)],
    all_columns: &BTreeMap<String, String>,
) -> DuckDbSourceResult<String> {
    if let Some(requested) = &layer.geometry_column {
        if geometry_columns.iter().any(|(name, _)| name == requested) {
            return Ok(requested.clone());
        }
        if let Some(column_type) = all_columns.get(requested) {
            return Err(DuckDbSourceError::NotGeometryColumn(
                requested.clone(),
                column_type.clone(),
            ));
        }
        return Err(DuckDbSourceError::GeometryColumnNotFound(requested.clone()));
    }

    match geometry_columns.len() {
        0 => Err(DuckDbSourceError::NoGeometryColumn),
        1 => Ok(geometry_columns[0].0.clone()),
        _ => Err(DuckDbSourceError::AmbiguousGeometryColumn(
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
) -> DuckDbSourceResult<BTreeMap<String, String>> {
    let query = format!("DESCRIBE SELECT * FROM {from_expr}");
    let query_for_error = query.clone();
    let source_label = source_label.to_owned();

    pool.generate_tile(move |conn| {
        Ok(conn.prepare(&query).and_then(|mut stmt| {
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            rows.collect::<Result<Vec<_>, _>>()
        }))
    })
    .await?
    .map_err(|source| {
        DuckDbSourceError::introspection_query(&source, source_label, "columns", query_for_error)
    })
    .map(|rows| rows.into_iter().collect())
}

async fn query_srid(
    pool: &DuckDBPool,
    from_expr: &str,
    source_label: &str,
    geometry_column: &str,
) -> DuckDbSourceResult<NonZeroI32> {
    let escaped_geometry_column = escape_identifier(geometry_column);
    let query = format!(
        "SELECT ST_CRS({escaped_geometry_column}) \
         FROM {from_expr} \
         WHERE {escaped_geometry_column} IS NOT NULL \
         LIMIT 1"
    );
    let query_for_error = query.clone();
    let source_label = source_label.to_owned();
    let geometry_column = geometry_column.to_owned();

    let crs = pool
        .generate_tile(move |conn| {
            use duckdb::OptionalExt as _;

            Ok(conn
                .query_row(&query, [], |row| row.get::<_, Option<String>>(0))
                .optional())
        })
        .await?
        .map_err(|source| {
            DuckDbSourceError::introspection_query(&source, source_label, "srid", query_for_error)
        })?;

    match crs.flatten() {
        None => Err(DuckDbSourceError::SridUnknown(geometry_column)),
        Some(crs) => parse_crs_to_srid(&crs, &geometry_column),
    }
}

pub(crate) fn parse_crs_to_srid(
    crs: &str,
    geometry_column: &str,
) -> DuckDbSourceResult<NonZeroI32> {
    let geometry_column = geometry_column.to_owned();
    let crs = crs.trim();
    if crs.is_empty() {
        return Err(DuckDbSourceError::SridEmpty(
            geometry_column,
            crs.to_owned(),
        ));
    }

    if crs.eq_ignore_ascii_case("OGC:CRS84") {
        return Ok(NonZeroI32::new(4326).expect("4326 is non-zero"));
    }

    let Some(auth_code) = crs
        .strip_prefix("EPSG:")
        .or_else(|| crs.strip_prefix("epsg:"))
    else {
        return Err(DuckDbSourceError::SridUnsupportedCrs(
            geometry_column,
            crs.to_owned(),
        ));
    };

    let srid = auth_code.parse::<i32>().map_err(|_err| {
        DuckDbSourceError::SridInvalidEpsgCode(geometry_column.clone(), crs.to_owned())
    })?;

    NonZeroI32::new(srid).ok_or(DuckDbSourceError::SridNonPositive(
        geometry_column,
        crs.to_owned(),
        srid,
    ))
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

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
        assert_matches!(err, DuckDbSourceError::SridUnsupportedCrs(..));
    }
}

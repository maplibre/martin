use std::collections::BTreeMap;

use martin_core::tiles::duckdb::DuckDBPool;
use serde::Deserialize;
use tracing::debug;

use crate::config::file::tiles::duckdb::sql_utils::escape_identifier;

/// SQL accessors for the four corners of a `GeoParquet` 1.1 `covering` bounding box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoveringBbox {
    pub xmin: String,
    pub ymin: String,
    pub xmax: String,
    pub ymax: String,
}

#[derive(Deserialize)]
struct GeoMetadata {
    #[serde(default)]
    columns: BTreeMap<String, GeoColumn>,
}

#[derive(Deserialize)]
struct GeoColumn {
    covering: Option<Covering>,
}

#[derive(Deserialize)]
struct Covering {
    bbox: CoveringPaths,
}

#[derive(Deserialize)]
struct CoveringPaths {
    xmin: Vec<String>,
    ymin: Vec<String>,
    xmax: Vec<String>,
    ymax: Vec<String>,
}

fn accessor(path: &[String]) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    Some(
        path.iter()
            .map(|part| escape_identifier(part))
            .collect::<Vec<_>>()
            .join("."),
    )
}

pub(crate) fn parse_covering(geo_metadata: &str, geometry_column: &str) -> Option<CoveringBbox> {
    let metadata = serde_json::from_str::<GeoMetadata>(geo_metadata).ok()?;
    let paths = metadata.columns.get(geometry_column)?.covering.as_ref()?;

    Some(CoveringBbox {
        xmin: accessor(&paths.bbox.xmin)?,
        ymin: accessor(&paths.bbox.ymin)?,
        xmax: accessor(&paths.bbox.xmax)?,
        ymax: accessor(&paths.bbox.ymax)?,
    })
}

/// Reads the `covering` declaration out of the Parquet `geo` key, if the file has one.
pub(crate) async fn query_covering(
    pool: &DuckDBPool,
    source_literal: &str,
    geometry_column: &str,
    source_label: &str,
) -> Option<CoveringBbox> {
    let query =
        format!("SELECT value FROM parquet_kv_metadata({source_literal}) WHERE key = 'geo'");
    let rows = pool
        .generate_tile(move |conn| {
            Ok(conn.prepare(&query).and_then(|mut stmt| {
                let rows = stmt.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
                rows.collect::<Result<Vec<_>, _>>()
            }))
        })
        .await;

    let rows = match rows {
        Ok(Ok(rows)) => rows,
        Ok(Err(source)) => {
            debug!("Could not read Parquet `geo` metadata of {source_label}: {source}");
            return None;
        }
        Err(source) => {
            debug!("Could not read Parquet `geo` metadata of {source_label}: {source}");
            return None;
        }
    };

    let covering = rows
        .iter()
        .find_map(|value| parse_covering(&String::from_utf8_lossy(value), geometry_column));

    if covering.is_none() {
        debug!(
            "{source_label} declares no GeoParquet `covering` for {geometry_column}; tile requests will scan every row group"
        );
    }
    covering
}

#[cfg(test)]
mod tests {
    use super::*;

    const OVERTURE_GEO: &str = r#"{"version": "1.1.0", "primary_column": "geometry", "columns": {"geometry": {"encoding": "WKB", "geometry_types": ["MultiPolygon", "Polygon"], "bbox": [-179.8, -67.5, -43.9, 83.3], "covering": {"bbox": {"xmin": ["bbox", "xmin"], "ymin": ["bbox", "ymin"], "xmax": ["bbox", "xmax"], "ymax": ["bbox", "ymax"]}}}}}"#;

    #[test]
    fn parse_covering_reads_the_overture_layout() {
        let covering = parse_covering(OVERTURE_GEO, "geometry").expect("covering");
        assert_eq!(
            covering,
            CoveringBbox {
                xmin: r#""bbox"."xmin""#.to_owned(),
                ymin: r#""bbox"."ymin""#.to_owned(),
                xmax: r#""bbox"."xmax""#.to_owned(),
                ymax: r#""bbox"."ymax""#.to_owned(),
            }
        );
    }

    #[test]
    fn parse_covering_does_not_assume_the_column_is_called_bbox() {
        let geo = r#"{"columns": {"geom": {"covering": {"bbox": {"xmin": ["envelope", "lo", "x"], "ymin": ["envelope", "lo", "y"], "xmax": ["envelope", "hi", "x"], "ymax": ["envelope", "hi", "y"]}}}}}"#;
        let covering = parse_covering(geo, "geom").expect("covering");
        assert_eq!(covering.xmin, r#""envelope"."lo"."x""#);
        assert_eq!(covering.ymax, r#""envelope"."hi"."y""#);
    }

    #[test]
    fn parse_covering_escapes_identifiers() {
        let geo = r#"{"columns": {"geom": {"covering": {"bbox": {"xmin": ["we\"ird"], "ymin": ["b"], "xmax": ["c"], "ymax": ["d"]}}}}}"#;
        let covering = parse_covering(geo, "geom").expect("covering");
        assert_eq!(covering.xmin, "\"we\"\"ird\"");
    }

    #[test]
    fn parse_covering_ignores_a_covering_on_another_column() {
        assert_eq!(parse_covering(OVERTURE_GEO, "geom"), None);
    }

    #[test]
    fn parse_covering_skips_geoparquet_1_0_metadata() {
        let geo = r#"{"version":"1.0.0","primary_column":"geom","columns":{"geom":{"encoding":"WKB","bbox":[-50.0,20.0,5.0,30.0]}}}"#;
        assert_eq!(parse_covering(geo, "geom"), None);
    }

    #[test]
    fn parse_covering_rejects_an_empty_path() {
        let geo = r#"{"columns": {"geom": {"covering": {"bbox": {"xmin": [], "ymin": ["b"], "xmax": ["c"], "ymax": ["d"]}}}}}"#;
        assert_eq!(parse_covering(geo, "geom"), None);
    }

    #[test]
    fn parse_covering_survives_metadata_that_is_not_json() {
        assert_eq!(parse_covering("not json at all", "geom"), None);
    }
}

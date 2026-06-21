use martin_core::tiles::duckdb::DuckDBError::PrepareQueryError;
use martin_core::tiles::duckdb::{DuckDBPool, DuckDBResult};
use tilejson::Bounds;
use tokio::time::timeout;
use tracing::warn;

use crate::config::args::{BoundsCalcType, DEFAULT_BOUNDS_TIMEOUT};
use crate::config::file::tiles::duckdb::sql_utils::{epsg_crs, escape_identifier};

fn escape_relation(relation: &str) -> String {
    relation
        .split('.')
        .map(escape_identifier)
        .collect::<Vec<_>>()
        .join(".")
}

async fn calc_exact_bounds(
    pool: &DuckDBPool,
    relation: &str,
    geom_col: &str,
    srid: i32,
) -> DuckDBResult<Option<Bounds>> {
    let escaped_relation = escape_relation(relation);
    let escaped_geom_col = escape_identifier(geom_col);
    let source_crs = epsg_crs(srid);
    let target_crs = epsg_crs(4326);
    let query = format!(
        r"WITH real_bounds AS (
    SELECT ST_Extent({escaped_geom_col}::GEOMETRY) AS ext
    FROM {escaped_relation}
)
SELECT
    ST_XMin(box) AS xmin,
    ST_YMin(box) AS ymin,
    ST_XMax(box) AS xmax,
    ST_YMax(box) AS ymax
FROM (
    SELECT ST_Transform(
        CASE
            WHEN (SELECT ST_XMin(ext) = ST_XMax(ext) OR ST_YMin(ext) = ST_YMax(ext)
                  FROM real_bounds LIMIT 1)
            THEN ST_Extent(ST_Buffer({escaped_geom_col}::GEOMETRY, 1))
            ELSE (SELECT ext FROM real_bounds LIMIT 1)
        END,
        {source_crs}, {target_crs}, always_xy := true
    ) AS box
    FROM {escaped_relation}
    LIMIT 1
) AS t;"
    );
    let source_id = relation.to_string();

    pool.generate_tile(move |conn| {
        conn.query_row(&query, [], |row| {
            let xmin: Option<f64> = row.get("xmin")?;
            let ymin: Option<f64> = row.get("ymin")?;
            let xmax: Option<f64> = row.get("xmax")?;
            let ymax: Option<f64> = row.get("ymax")?;

            Ok(match (xmin, ymin, xmax, ymax) {
                (Some(xmin), Some(ymin), Some(xmax), Some(ymax)) => {
                    Some(Bounds::new(xmin, ymin, xmax, ymax))
                }
                _ => None,
            })
        })
        .map_err(|source| PrepareQueryError {
            source: source.into(),
            source_id,
            signature: "bounds".to_string(),
            query,
        })
    })
    .await
}

pub async fn calc_relation_bounds(
    pool: &DuckDBPool,
    relation: &str,
    geom_col: &str,
    srid: i32,
    auto_bounds: BoundsCalcType,
) -> DuckDBResult<Option<Bounds>> {
    match auto_bounds {
        BoundsCalcType::Skip => Ok(None),
        BoundsCalcType::Calc => calc_exact_bounds(pool, relation, geom_col, srid).await,
        BoundsCalcType::Quick => {
            match timeout(
                DEFAULT_BOUNDS_TIMEOUT,
                calc_exact_bounds(pool, relation, geom_col, srid),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => {
                    warn!(
                        "Timeout computing bounds for {relation}, aborting query. Use --auto-bounds=calc to wait until complete."
                    );
                    Ok(None)
                }
            }
        }
    }
}

#[cfg(test)]
#[cfg(feature = "unstable-duckdb")]
mod tests {
    use tilejson::Bounds;

    use super::calc_relation_bounds;
    use crate::config::args::BoundsCalcType;
    use crate::test_support::duckdb::TestDatabase;

    #[tokio::test]
    async fn calc_and_skip_relation_bounds() {
        let db = TestDatabase::from_sql(
            "bounds.duckdb",
            include_str!("../../../../../../../tests/fixtures/duckdb/bounds_point.sql"),
        );
        let pool = db.read_only_pool("bounds-test", 1);

        let calc = calc_relation_bounds(&pool, "test_geom", "geom", 4326, BoundsCalcType::Calc)
            .await
            .expect("calculate bounds");
        assert_eq!(calc, Some(Bounds::new(9.0, 19.0, 11.0, 21.0)));

        let skip = calc_relation_bounds(&pool, "test_geom", "geom", 4326, BoundsCalcType::Skip)
            .await
            .expect("skip bounds");
        assert_eq!(skip, None);
    }
}

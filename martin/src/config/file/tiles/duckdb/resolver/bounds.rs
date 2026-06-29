use martin_core::tiles::duckdb::DuckDBPool;
use tilejson::Bounds;
use tokio::time::timeout;
use tracing::{debug, warn};

use crate::config::args::{BoundsCalcType, DEFAULT_BOUNDS_TIMEOUT};
use crate::config::file::tiles::duckdb::resolver::error::{BoundsError, BoundsResult};
use crate::config::file::tiles::duckdb::sql_utils::{epsg_crs, escape_identifier};

/// How [`calc_bounds`] should compute a geometry bounds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundsCalcMode {
    /// Exact `ST_Extent` over the whole relation. Accurate, but potentially slow on large tables.
    Exact,
    /// Per-row `ST_Extent_Approx` using cached geometry bounding boxes, falling back to
    /// [`Self::Exact`] when unavailable. Unlike PostGIS `ST_EstimatedExtent`, this still scans
    /// the relation but is cheaper per row.
    Estimate,
}

fn escape_relation(relation: &str) -> String {
    relation
        .split('.')
        .map(escape_identifier)
        .collect::<Vec<_>>()
        .join(".")
}

async fn fetch_bounds(
    pool: &DuckDBPool,
    relation: &str,
    query: String,
    signature: &str,
) -> BoundsResult<Option<Bounds>> {
    let relation = relation.to_string();
    let signature = signature.to_string();
    let query_for_error = query.clone();

    pool.generate_tile(move |conn| {
        Ok(conn.query_row(&query, [], |row| {
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
        }))
    })
    .await?
    .map_err(|source| BoundsError::Query {
        source: source.into(),
        relation,
        signature,
        query: query_for_error,
    })
}

async fn calc_bounds(
    pool: &DuckDBPool,
    from_sql: &str,
    label: &str,
    geom_col: &str,
    srid: i32,
    mode: BoundsCalcMode,
) -> BoundsResult<Option<Bounds>> {
    let escaped_geom_col = escape_identifier(geom_col);
    let source_crs = epsg_crs(srid);
    let target_crs = epsg_crs(4326);

    if mode == BoundsCalcMode::Estimate {
        // ST_Extent_Approx reads each geometry's cached bounding box instead of computing the
        // full extent, but still scans the relation. Any failure (missing cached boxes, an
        // unavailable function, or a query error) falls back to the exact calculation rather
        // than aborting.
        let query = format!(
            r"WITH row_boxes AS (
    SELECT ST_Extent_Approx({escaped_geom_col}::GEOMETRY) AS box
    FROM {from_sql}
),
merged AS (
    SELECT
        min(ST_XMin(box)) AS xmin,
        min(ST_YMin(box)) AS ymin,
        max(ST_XMax(box)) AS xmax,
        max(ST_YMax(box)) AS ymax,
        count(box) AS box_count
    FROM row_boxes
    WHERE box IS NOT NULL
)
SELECT
    ST_XMin(out_box) AS xmin,
    ST_YMin(out_box) AS ymin,
    ST_XMax(out_box) AS xmax,
    ST_YMax(out_box) AS ymax
FROM (
    SELECT ST_Transform(
        CASE
            WHEN (SELECT box_count FROM merged) = 0 THEN NULL
            WHEN (SELECT xmin = xmax OR ymin = ymax FROM merged)
            THEN {{
                min_x: (SELECT xmin - 1 FROM merged),
                min_y: (SELECT ymin - 1 FROM merged),
                max_x: (SELECT xmax + 1 FROM merged),
                max_y: (SELECT ymax + 1 FROM merged)
            }}::BOX_2D
            ELSE {{
                min_x: (SELECT xmin FROM merged),
                min_y: (SELECT ymin FROM merged),
                max_x: (SELECT xmax FROM merged),
                max_y: (SELECT ymax FROM merged)
            }}::BOX_2D
        END,
        {source_crs}, {target_crs}, always_xy := true
    ) AS out_box
) AS t
WHERE out_box IS NOT NULL;"
        );

        match fetch_bounds(pool, label, query, "approx-bounds").await {
            Ok(Some(bounds)) => return Ok(Some(bounds)),
            Ok(None) => {
                debug!(
                    "ST_Extent_Approx on {label}.{geom_col} returned no bounds, trying exact calculation"
                );
            }
            Err(_) => {
                warn!(
                    "ST_Extent_Approx on {label}.{geom_col} failed, trying slower method to compute bounds"
                );
            }
        }
    }

    let query = format!(
        r"WITH real_bounds AS (
    SELECT ST_Extent({escaped_geom_col}::GEOMETRY) AS ext
    FROM {from_sql}
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
    FROM {from_sql}
    LIMIT 1
) AS t;"
    );

    fetch_bounds(pool, label, query, "bounds").await
}

pub async fn bounds_with_auto(
    pool: &DuckDBPool,
    from_sql: &str,
    label: &str,
    geom_col: &str,
    srid: i32,
    auto_bounds: BoundsCalcType,
) -> BoundsResult<Option<Bounds>> {
    match auto_bounds {
        BoundsCalcType::Skip => Ok(None),
        BoundsCalcType::Calc => {
            calc_bounds(pool, from_sql, label, geom_col, srid, BoundsCalcMode::Exact).await
        }
        BoundsCalcType::Quick => {
            match timeout(
                DEFAULT_BOUNDS_TIMEOUT,
                calc_bounds(
                    pool,
                    from_sql,
                    label,
                    geom_col,
                    srid,
                    BoundsCalcMode::Estimate,
                ),
            )
            .await
            {
                Ok(bounds) => bounds,
                Err(_) => {
                    warn!(
                        "Timeout computing bounds for {label}, aborting query. Use --auto-bounds=calc to wait until complete."
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

    use super::{bounds_with_auto, escape_relation};
    use crate::config::args::BoundsCalcType;
    use crate::test_support::duckdb::TestDatabase;

    #[tokio::test(flavor = "multi_thread")]
    async fn calc_and_skip_relation_bounds() {
        let db = TestDatabase::from_sql(
            "bounds.duckdb",
            include_str!("../../../../../../../tests/fixtures/duckdb/bounds_point.sql"),
        );
        let pool = db.read_only_pool("bounds-test", 1);
        let from_sql = escape_relation("test_geom");

        let calc = bounds_with_auto(
            &pool,
            &from_sql,
            "test_geom",
            "geom",
            4326,
            BoundsCalcType::Calc,
        )
        .await
        .expect("calculate bounds");
        assert_eq!(calc, Some(Bounds::new(9.0, 19.0, 11.0, 21.0)));

        let quick = bounds_with_auto(
            &pool,
            &from_sql,
            "test_geom",
            "geom",
            4326,
            BoundsCalcType::Quick,
        )
        .await
        .expect("approx bounds");
        assert_eq!(quick, Some(Bounds::new(9.0, 19.0, 11.0, 21.0)));

        let skip = bounds_with_auto(
            &pool,
            &from_sql,
            "test_geom",
            "geom",
            4326,
            BoundsCalcType::Skip,
        )
        .await
        .expect("skip bounds");
        assert_eq!(skip, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn bounds_with_auto_over_read_parquet() {
        use crate::test_support::duckdb::TestGeoParquet;

        let fixture = TestGeoParquet::from_sql(
            "bounds.parquet",
            include_str!("../../../../../../../tests/fixtures/duckdb/geoparquet_points.sql"),
            "points",
        );
        let pool = fixture.query_pool("bounds-parquet", 1);
        let from_expr = format!(
            "read_parquet('{}')",
            fixture.path().to_str().expect("utf-8 parquet path")
        );

        let bounds = bounds_with_auto(
            &pool,
            &from_expr,
            "points.parquet",
            "geom",
            4326,
            BoundsCalcType::Calc,
        )
        .await
        .expect("parquet bounds");

        assert_eq!(bounds, Some(Bounds::new(9.0, 19.0, 11.0, 21.0)));
    }
}

-- sqlfluff:dialect:duckdb

-- Overture-style linestrings with a `bbox` struct column. DuckDB 2.0 reads the geometry chunk
-- (over 1 MiB) of such a remote file past its external file cache on every query.

-- Regenerate (from repo root): duckdb -c "INSTALL spatial; LOAD spatial;"
--   -c "$(cat tests/fixtures/duckdb/geoparquet_bbox_column.sql)"
COPY (
    SELECT
        id,
        geom,
        {
            'xmin': ST_XMIN(geom), 'xmax': ST_XMAX(geom), 'ymin': ST_YMIN(geom), 'ymax': ST_YMAX(geom)
        } AS bbox
    FROM (
        SELECT
            t.r AS id,
            ST_SETCRS(
                ST_MAKELINE(
                    LIST(
                        ST_POINT(
                            -10 + (t.r * 7919 % 20000) / 1000.0 + s.j * 0.000137 * (t.r % 7 + 1),
                            20 + (t.r * 104729 % 6000) / 1000.0 + s.j * 0.000173 * (t.r % 5 + 1)
                        )
                        ORDER BY s.j
                    )
                ),
                'EPSG:4326'
            ) AS geom
        FROM RANGE(0, 16384) AS t (r), RANGE(0, 5) AS s (j) -- noqa: AL05
        GROUP BY t.r
        ORDER BY t.r
    ) AS linestrings
) TO 'tests/fixtures/duckdb/geoparquet_bbox_column.parquet' ( -- noqa: PRS
    FORMAT parquet
);

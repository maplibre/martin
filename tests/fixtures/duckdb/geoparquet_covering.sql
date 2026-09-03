-- sqlfluff:dialect:duckdb

-- A GeoParquet with GeoParquet 1.1 `covering` metadata, spatially sorted into small row
-- groups so that a tile request can skip most of them. Points only span the western
-- hemisphere, so a tile over the eastern one must read no row group at all.
--
-- Regenerate geoparquet_covering.parquet (from repo root):
--   duckdb -c "INSTALL spatial; LOAD spatial;" \
--          -c "$(cat tests/fixtures/duckdb/geoparquet_covering.sql)"
CREATE TABLE covering_points AS
SELECT
    i AS id,
    (-179.99 + i * 0.0087)::DOUBLE AS lon,
    (-60.0 + (i % 121) * 1.0)::DOUBLE AS lat
FROM range(0, 20480) AS t (i); -- noqa: AL05

COPY (
    SELECT
        id,
        {
            'xmin': lon, 'ymin': lat, 'xmax': lon, 'ymax': lat
        }::STRUCT(xmin DOUBLE, ymin DOUBLE, xmax DOUBLE, ymax DOUBLE) AS bbox,
        ST_ASWKB(ST_POINT(lon, lat)) AS geom -- noqa: CP03
    FROM covering_points
    ORDER BY lon
) TO 'tests/fixtures/duckdb/geoparquet_covering.parquet' ( -- noqa: PRS
    FORMAT PARQUET,
    ROW_GROUP_SIZE 2048,
    KV_METADATA {
        geo: '{"version":"1.1.0","primary_column":"geom","columns":{"geom":{"encoding":"WKB","geometry_types":["Point"],"crs":null,"covering":{"bbox":{"xmin":["bbox","xmin"],"ymin":["bbox","ymin"],"xmax":["bbox","xmax"],"ymax":["bbox","ymax"]}}}}}'
    }
);

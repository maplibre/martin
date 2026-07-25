-- Deterministic WGS84 polygon fixture for DuckDB GeoParquet E2E.
-- `boundary_span` crosses lon=0 so it intersects both z1 tiles 1/0/0 and 1/1/0.
-- `inside_west` sits wholly inside tile 1/0/0 so that tile's MVT has two features.

-- sqlfluff:dialect:duckdb

-- Regenerate geoparquet_polygons.parquet (from repo root):
--   duckdb -c "INSTALL spatial; LOAD spatial;" \
--          -c "$(cat tests/fixtures/duckdb/geoparquet_polygons.sql)" \
--          -c "COPY polygons TO 'tests/fixtures/duckdb/geoparquet_polygons.parquet' (FORMAT PARQUET);"
CREATE TABLE polygons (
    id INTEGER,
    name VARCHAR,
    geom GEOMETRY
);

INSERT INTO polygons VALUES
(
    1,
    'boundary_span',
    ST_SETCRS(
        ST_GEOMFROMTEXT('POLYGON((-5 20, 5 20, 5 30, -5 30, -5 20))'),
        'EPSG:4326'
    )
),
(
    2,
    'inside_west',
    ST_SETCRS(
        ST_GEOMFROMTEXT('POLYGON((-50 20, -40 20, -40 30, -50 30, -50 20))'),
        'EPSG:4326'
    )
);

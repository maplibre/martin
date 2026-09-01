-- sqlfluff:dialect:duckdb

-- Regenerate geoparquet_mixed_types.parquet (from repo root):
--   duckdb -c "INSTALL spatial; LOAD spatial;" \
--          -c "$(cat tests/fixtures/duckdb/geoparquet_mixed_types.sql)" \
--          -c "COPY mixed_types TO 'tests/fixtures/duckdb/geoparquet_mixed_types.parquet' (FORMAT PARQUET);"
CREATE TABLE mixed_types (
    id INTEGER,
    name VARCHAR,
    visitors SMALLINT,
    ratio DECIMAL(10, 4),
    opened DATE,
    tags VARCHAR[],
    address STRUCT(street VARCHAR, city VARCHAR),
    thumbnail BLOB,
    geom GEOMETRY
);

INSERT INTO mixed_types VALUES
(
    1,
    'boundary_span',
    120,
    0.5,
    DATE '2020-01-02',
    ['a', 'b'],
    {'street': 'Main', 'city': 'Springfield'},
    '\xDE\xAD'::BLOB,
    ST_SETCRS(
        ST_GEOMFROMTEXT('POLYGON((-5 20, 5 20, 5 30, -5 30, -5 20))'),
        'EPSG:4326'
    )
),
(
    2,
    'inside_west',
    -3,
    12.25,
    DATE '2021-06-30',
    ['c'],
    {'street': 'Elm', 'city': 'Shelbyville'},
    '\xBE\xEF'::BLOB,
    ST_SETCRS(
        ST_GEOMFROMTEXT('POLYGON((-50 20, -40 20, -40 30, -50 30, -50 20))'),
        'EPSG:4326'
    )
);

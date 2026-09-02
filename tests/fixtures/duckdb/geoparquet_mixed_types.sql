-- sqlfluff:dialect:duckdb

-- Regenerate geoparquet_mixed_types.parquet (from repo root):
--   duckdb -c "INSTALL spatial; LOAD spatial;" \
--          -c "$(cat tests/fixtures/duckdb/geoparquet_mixed_types.sql)" \
--          -c "COPY mixed_types TO 'tests/fixtures/duckdb/geoparquet_mixed_types.parquet' (FORMAT PARQUET);"
CREATE TABLE mixed_types (
    id INTEGER,
    name VARCHAR,
    visitors SMALLINT,
    floor TINYINT,
    stars UTINYINT,
    capacity USMALLINT,
    population UINTEGER,
    endowment BIGINT,
    catalog_ref UBIGINT,
    ratio DECIMAL(10, 4),
    rating FLOAT,
    area DOUBLE,
    is_open BOOLEAN,
    opened DATE,
    opens_at TIME,
    closes_at TIME WITH TIME ZONE,
    surveyed_at TIMESTAMP,
    published_at TIMESTAMP WITH TIME ZONE,
    ingested_at TIMESTAMP_NS,
    tour_length INTERVAL,
    external_id UUID,
    details JSON,
    thumbnail BLOB,
    tags VARCHAR[],
    address STRUCT(street VARCHAR, city VARCHAR),
    attributes MAP(VARCHAR, VARCHAR),
    centroid GEOMETRY,
    geom GEOMETRY
);

INSERT INTO mixed_types VALUES
(
    1,
    'boundary_span',
    120,
    -128,
    0,
    65535,
    4294967295,
    9223372036854775807,
    18446744073709551615,
    0.5,
    4.5,
    1234.5678,
    true,
    DATE '2020-01-02',
    TIME '08:30:00',
    TIMETZ '17:45:15+02',
    TIMESTAMP '2020-01-02 03:04:05',
    TIMESTAMPTZ '2020-01-02 03:04:05+00',
    TIMESTAMP_NS '2020-01-02 03:04:05.123456789',
    INTERVAL 90 MINUTES,
    UUID '5b8d1a4e-1e6c-4c6f-9b1a-2f0d3c4b5a69',
    '{"kind":"park"}',
    '\xDE\xAD'::BLOB,
    ['a', 'b'],
    {'street': 'Main', 'city': 'Springfield'},
    MAP {'wheelchair': 'yes'},
    ST_SETCRS(ST_GEOMFROMTEXT('POINT(0 25)'), 'EPSG:4326'),
    ST_SETCRS(
        ST_GEOMFROMTEXT('POLYGON((-5 20, 5 20, 5 30, -5 30, -5 20))'),
        'EPSG:4326'
    )
),
(
    2,
    'inside_west',
    -3,
    127,
    255,
    1,
    7,
    -9223372036854775808,
    0,
    12.25,
    -2.25,
    -0.125,
    false,
    DATE '2021-06-30',
    TIME '17:45:15',
    TIMETZ '08:30:00-05',
    TIMESTAMP '2021-06-30 23:59:59',
    TIMESTAMPTZ '2021-06-30 23:59:59+00',
    TIMESTAMP_NS '2021-06-30 23:59:59.987654321',
    INTERVAL 3 DAYS,
    UUID '9f14c3d2-7b0a-4d5e-8c11-6a2b3e4f5061',
    '{"kind":"museum"}',
    '\xBE\xEF'::BLOB,
    ['c'],
    {'street': 'Elm', 'city': 'Shelbyville'},
    MAP {'wheelchair': 'no'},
    ST_SETCRS(ST_GEOMFROMTEXT('POINT(-45 25)'), 'EPSG:4326'),
    ST_SETCRS(
        ST_GEOMFROMTEXT('POLYGON((-50 20, -40 20, -40 30, -50 30, -50 20))'),
        'EPSG:4326'
    )
);

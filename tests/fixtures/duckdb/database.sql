-- sqlfluff:dialect:duckdb

-- Regenerate database.duckdb (from repo root):
--   rm -f tests/fixtures/duckdb/database.duckdb
--   duckdb -c "INSTALL spatial; LOAD spatial;" \
--          -c "ATTACH 'tests/fixtures/duckdb/database.duckdb' AS db (STORAGE_VERSION 'v1.5.0'); USE db;" \
--          -c "$(cat tests/fixtures/duckdb/database.sql)"
CREATE TABLE polygons (
    id INTEGER,
    name VARCHAR,
    geom GEOMETRY('EPSG:4326')
);

INSERT INTO polygons VALUES
(
    1,
    'boundary_span',
    ST_GEOMFROMTEXT('POLYGON((-5 20, 5 20, 5 30, -5 30, -5 20))')
),
(
    2,
    'inside_west',
    ST_GEOMFROMTEXT('POLYGON((-50 20, -40 20, -40 30, -50 30, -50 20))')
);

CREATE TABLE plain (
    id INTEGER,
    name VARCHAR
);

INSERT INTO plain VALUES (1, 'no geometry here');

CREATE SCHEMA places;

CREATE TABLE places.points (
    gid INTEGER,
    label VARCHAR,
    geom GEOMETRY('EPSG:4326')
);

INSERT INTO places.points VALUES
(1, 'west', ST_POINT(-45, 25)),
(2, 'east', ST_POINT(0, 25));

CREATE MACRO polygons_mvt(z, x, y) AS TABLE
SELECT ST_ASMVT(tile, 'polygons_mvt', 4096, 'geom') AS mvt
FROM (
    SELECT
        ST_ASMVTGEOM(
            ST_TRANSFORM(geom, 'EPSG:4326', 'EPSG:3857', always_xy := true),
            ST_EXTENT(ST_TILEENVELOPE(z, x, y)),
            4096,
            64,
            true
        ) AS geom,
        id,
        name
    FROM polygons
    WHERE
        ST_INTERSECTS(
            ST_TRANSFORM(geom, 'EPSG:4326', 'EPSG:3857', always_xy := true),
            ST_TILEENVELOPE(z, x, y)
        )
) AS tile;

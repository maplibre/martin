CREATE TABLE table_source_multiple_geom (
    gid serial PRIMARY KEY,
    geom1 GEOMETRY(point, 4326),
    geom2 GEOMETRY(point, 4326)
);

INSERT INTO table_source_multiple_geom
SELECT
    generate_series(1, 10000) AS id,
    (ST_DUMP (ST_GENERATEPOINTS (ST_GEOMFROMTEXT ('POLYGON ((-180 90, 180 90, 180 -90, -180 -90, -180 90))', 4326), 10000))).geom,
    (ST_DUMP (ST_GENERATEPOINTS (ST_GEOMFROMTEXT ('POLYGON ((-180 90, 180 90, 180 -90, -180 -90, -180 90))', 4326), 10000))).geom;

CREATE INDEX ON table_source_multiple_geom USING GIST (geom1);
CREATE INDEX ON table_source_multiple_geom USING GIST (geom2);

CLUSTER table_source_multiple_geom_geom1_idx ON table_source_multiple_geom;
CLUSTER table_source_multiple_geom_geom2_idx ON table_source_multiple_geom;

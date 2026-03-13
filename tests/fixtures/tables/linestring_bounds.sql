-- This table reproduces the issue where ST_Extent returns a LineString
-- instead of a Polygon when all geometries are collinear.
-- See: https://github.com/maplibre/martin/issues/XXXX
CREATE TABLE linestring_bounds
(
    gid SERIAL PRIMARY KEY,
    geom GEOMETRY(Geometry, 4326)
);

INSERT INTO linestring_bounds (geom)
VALUES ('SRID=4326;LINESTRING (9.9581704 10.0370178, 9.9675324 10.0370178)');

CREATE INDEX ON linestring_bounds USING gist (geom);

-- Table with a single vertical LineString
CREATE TABLE linestring_bounds_vertical
(
    gid SERIAL PRIMARY KEY,
    geom GEOMETRY (GEOMETRY, 4326)
);

INSERT INTO linestring_bounds_vertical (geom)
VALUES ('SRID=4326;LINESTRING (10.0 9.9581704, 10.0 9.9675324)');

CREATE INDEX ON linestring_bounds_vertical USING gist (geom);

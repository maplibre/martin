-- Table with a single vertical LineString (same x, different y coordinates).
-- ST_Extent on this data returns a vertical ST_LineString, which previously
-- caused Martin to crash at startup.
CREATE TABLE linestring_bounds_vertical
(
    gid SERIAL PRIMARY KEY,
    geom GEOMETRY(Geometry, 4326)
);

INSERT INTO linestring_bounds_vertical (geom)
VALUES ('SRID=4326;LINESTRING (10.0 9.9581704, 10.0 9.9675324)');

CREATE INDEX ON linestring_bounds_vertical USING gist (geom);

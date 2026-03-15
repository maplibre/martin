-- ST_Extent returns a LineString
CREATE TABLE linestring_bounds
(
    gid SERIAL PRIMARY KEY,
    geom GEOMETRY (GEOMETRY, 4326)
);

INSERT INTO linestring_bounds (geom)
VALUES ('SRID=4326;LINESTRING (9.9581704 10.0370178, 9.9675324 10.0370178)');

CREATE INDEX ON linestring_bounds USING gist (geom);

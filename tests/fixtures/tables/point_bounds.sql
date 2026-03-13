-- Table with a single point geometry
CREATE TABLE point_bounds
(
    gid SERIAL PRIMARY KEY,
    geom GEOMETRY(Geometry, 4326)
);

INSERT INTO point_bounds (geom)
VALUES ('SRID=4326;POINT (10.0 20.0)');

CREATE INDEX ON point_bounds USING gist (geom);

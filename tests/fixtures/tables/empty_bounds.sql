-- Table with no rows
CREATE TABLE empty_bounds
(
    gid SERIAL PRIMARY KEY,
    geom GEOMETRY (GEOMETRY, 4326)
);

CREATE INDEX ON empty_bounds USING gist (geom);

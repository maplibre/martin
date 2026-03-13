-- Table with no rows. ST_Extent on an empty table returns NULL, so bounds
-- should be None without any crash.
CREATE TABLE empty_bounds
(
    gid SERIAL PRIMARY KEY,
    geom GEOMETRY(Geometry, 4326)
);

CREATE INDEX ON empty_bounds USING gist (geom);

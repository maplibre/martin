-- This table is intentionally left empty
CREATE TABLE points_empty
(
    gid SERIAL PRIMARY KEY,
    geom GEOMETRY
);

CREATE INDEX ON points_empty USING gist (geom);
CLUSTER points_empty_geom_idx ON points_empty;

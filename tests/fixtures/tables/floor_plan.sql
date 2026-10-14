-- Rooms of a floor plan in plain planar units (SRID 0), for serving a table on a simple tile grid.
CREATE TABLE floor_plan
(
    gid SERIAL PRIMARY KEY,
    room TEXT,
    geom GEOMETRY (POINT, 0)
);

INSERT INTO floor_plan (room, geom)
VALUES ('Lobby', ST_MAKEPOINT(120, 880)),
('Kitchen', ST_MAKEPOINT(700, 900)),
('Server room', ST_MAKEPOINT(650, 150));

CREATE INDEX ON floor_plan USING gist (geom);

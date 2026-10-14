-- Three New Zealand cities in NZTM2000 (EPSG:2193), for serving a table in the NZTM2000Quad tile grid.
CREATE TABLE nz_points
(
    gid SERIAL PRIMARY KEY,
    city TEXT,
    geom GEOMETRY (POINT, 2193)
);

INSERT INTO nz_points (city, geom)
VALUES ('Auckland', ST_SETSRID(ST_MAKEPOINT(1757000, 5920000), 2193)),
('Wellington', ST_SETSRID(ST_MAKEPOINT(1749000, 5428000), 2193)),
('Christchurch', ST_SETSRID(ST_MAKEPOINT(1570000, 5180000), 2193));

CREATE INDEX ON nz_points USING gist (geom);

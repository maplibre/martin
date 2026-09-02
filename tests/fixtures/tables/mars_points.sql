-- A coordinate reference system PostGIS does not ship: Mars 2015 planetocentric longitude/latitude,
-- registered the way planetary users do it, as a spatial_ref_sys row under the IAU authority.
INSERT INTO spatial_ref_sys (srid, auth_name, auth_srid, proj4text, srtext)
VALUES (949900, 'IAU_2015', 49900, '+proj=longlat +a=3396190 +b=3376200 +no_defs +type=crs', NULL);

-- Landing sites, for serving a table in a tile grid outside EPSG.
CREATE TABLE mars_points
(
    gid SERIAL PRIMARY KEY,
    site TEXT,
    geom GEOMETRY (POINT, 949900)
);

INSERT INTO mars_points (site, geom)
VALUES ('Viking 1', ST_SETSRID(ST_MAKEPOINT(-48.222, 22.697), 949900)),
('Curiosity', ST_SETSRID(ST_MAKEPOINT(137.442, -4.589), 949900)),
('Perseverance', ST_SETSRID(ST_MAKEPOINT(77.451, 18.445), 949900));

CREATE INDEX ON mars_points USING gist (geom);

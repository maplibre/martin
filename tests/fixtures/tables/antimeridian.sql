DROP TABLE IF EXISTS antimeridian;

CREATE TABLE antimeridian
(
    gid serial PRIMARY KEY,
    geom GEOMETRY (GEOMETRY, 4326)
);

INSERT INTO antimeridian (geom) VALUES (
    GEOMFROMEWKT('SRID=4326;POLYGON((-177 51, -172 51, -172 53, -177 53, -177 51))')
);
INSERT INTO antimeridian (geom) VALUES (
    GEOMFROMEWKT('SRID=4326;POLYGON((-182 51, -177 51, -177 53, -182 53, -182 51))')
);
INSERT INTO antimeridian (geom) VALUES (
    GEOMFROMEWKT('SRID=4326;POLYGON ((-160 60, -165 60, -165 62, -160 62, -160 60))')
);

CREATE INDEX ON antimeridian USING gist (geom);

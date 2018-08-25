DROP TABLE IF EXISTS points;
CREATE TABLE points(gid serial PRIMARY KEY, geom geometry);
INSERT INTO points(geom) values (GeomFromEWKT('SRID=4326;POINT(0 0)'));
INSERT INTO points(geom) values (GeomFromEWKT('SRID=4326;POINT(-2 2)'));

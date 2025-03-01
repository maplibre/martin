DROP TABLE IF EXISTS table_source_geog;
CREATE TABLE table_source_geog(gid serial PRIMARY KEY, geog geography(GEOMETRY, 4326));

INSERT INTO table_source_geog(geog) values ('SRID=4326;POINT(0 0)'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;POINT(-2 2)'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;LINESTRING(0 0, 1 1)'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;LINESTRING(2 2, 3 3)'::geography);

INSERT INTO table_source_geog(geog) values ('SRID=4326;POINT(30 10)'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;LINESTRING(30 10, 10 30, 40 40)'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;POLYGON((30 10, 40 40, 20 40, 10 20, 30 10))'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;POLYGON((35 10, 45 45, 15 40, 10 20, 35 10),(20 30, 35 35, 30 20, 20 30))'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;MULTIPOINT((10 40), (40 30), (20 20), (30 10))'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;MULTIPOINT(10 40, 40 30, 20 20, 30 10)'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;MULTILINESTRING((10 10, 20 20, 10 40),(40 40, 30 30, 40 20, 30 10))'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;MULTIPOLYGON(((30 20, 45 40, 10 40, 30 20)),((15 5, 40 10, 10 20, 5 10, 15 5)))'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;MULTIPOLYGON(((40 40, 20 45, 45 30, 40 40)),((20 35, 10 30, 10 10, 30 5, 45 20, 20 35),(30 20, 20 15, 20 25, 30 20)))'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;GEOMETRYCOLLECTION(POINT(4 6),LINESTRING(4 6,7 10))'::geography);

-- Curves are not supported in geography columns
-- INSERT INTO table_source_geog(geog) values ('SRID=4326;CIRCULARSTRING(1 5, 6 2, 7 3)'::geography);
-- INSERT INTO table_source_geog(geog) values ('SRID=4326;COMPOUNDCURVE(CIRCULARSTRING(0 0,1 1,1 0),(1 0,0 1))'::geography);
-- INSERT INTO table_source_geog(geog) values ('SRID=4326;CURVEPOLYGON(CIRCULARSTRING(-2 0,-1 -1,0 0,1 -1,2 0,0 2,-2 0),(-1 0,0 0.5,1 0,0 1,-1 0))'::geography);
-- INSERT INTO table_source_geog(geog) values ('SRID=4326;MULTICURVE((5 5,3 5,3 3,0 3),CIRCULARSTRING(0 0,2 1,2 2))'::geography);

INSERT INTO table_source_geog(geog) values ('SRID=4326;POINT(142.84124343269863 11.927545216212339)'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;POINT(142.84022627741408 11.926919775099435)'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;POINT(142.84116724279622 11.926986082398354)'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;POINT(142.84129834730146 11.926483025982757)'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;POINT(142.84086326293937 11.92741281580712)'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;POINT(142.84083973422645 11.927188724740008)'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;POINT(142.8407405154705 11.92659842381238)'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;POINT(142.84029057105903 11.92711170365923)'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;POINT(142.8403402985401 11.927568375227375)'::geography);
INSERT INTO table_source_geog(geog) values ('SRID=4326;POINT(142.84131509869133 11.92781306544329)'::geography);

-- DO NOT CREATE INDEX ON GEOGRAPHY COLUMN -- this table is used in a test case

DO $do$ BEGIN
    EXECUTE 'COMMENT ON TABLE table_source_geog IS $tj$' || $$
    {
        "description": null,
        "foo": {"bar": "foo"}
    }
    $$::json || '$tj$';
END $do$;

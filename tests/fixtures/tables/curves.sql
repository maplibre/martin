-- Curve geometries, which ST_AsMVTGeom cannot encode without ST_CurveToLine first.
-- One column is typed as a curve, one is the generic GEOMETRY type holding a curve.
DROP TABLE IF EXISTS curves;
CREATE TABLE curves (
    gid serial PRIMARY KEY, geom GEOMETRY (CURVEPOLYGON, 4326)
);
INSERT INTO curves (geom) VALUES (
    GEOMFROMEWKT('SRID=4326;CURVEPOLYGON(CIRCULARSTRING(0 0, 4 0, 4 4, 0 4, 0 0))')
);
INSERT INTO curves (geom) VALUES (
    GEOMFROMEWKT('SRID=4326;CURVEPOLYGON(CIRCULARSTRING(10 10, 14 10, 14 14, 10 14, 10 10))')
);
CREATE INDEX CONCURRENTLY ON curves USING gist (geom);

DROP TABLE IF EXISTS curves_untyped;
CREATE TABLE curves_untyped (
    gid serial PRIMARY KEY, geom GEOMETRY (GEOMETRY, 4326)
);
INSERT INTO curves_untyped (geom) VALUES (
    GEOMFROMEWKT('SRID=4326;CIRCULARSTRING(0 0, 1 1, 2 0)')
);
INSERT INTO curves_untyped (geom) VALUES (
    GEOMFROMEWKT('SRID=4326;COMPOUNDCURVE(CIRCULARSTRING(0 0, 1 1, 1 0), (1 0, 0 1))')
);
CREATE INDEX CONCURRENTLY ON curves_untyped USING gist (geom);

CREATE TABLE two_geoms (
    id INTEGER,
    geom_a GEOMETRY,
    geom_b GEOMETRY
);

INSERT INTO two_geoms VALUES
(
    1,
    ST_SETCRS(ST_POINT(10, 20), 'EPSG:4326'),
    ST_SETCRS(ST_POINT(12, 22), 'EPSG:4326')
);

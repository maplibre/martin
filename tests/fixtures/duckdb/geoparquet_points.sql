CREATE TABLE points (
    id INTEGER,
    point_name VARCHAR,
    geom GEOMETRY
);

INSERT INTO points VALUES
(1, 'alpha', ST_SETCRS(ST_POINT(10, 20), 'EPSG:4326')),
(2, 'beta', ST_SETCRS(ST_POINT(11, 21), 'EPSG:4326'));

CREATE TABLE points (
    id INTEGER,
    name VARCHAR,
    geom GEOMETRY
);

INSERT INTO points VALUES
    (1, 'alpha', ST_SetCRS(ST_Point(10, 20), 'EPSG:4326')),
    (2, 'beta', ST_SetCRS(ST_Point(11, 21), 'EPSG:4326'));

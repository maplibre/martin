create table points(gid serial PRIMARY KEY, geom geometry(GEOMETRY, 4326));

INSERT INTO points
    SELECT
        generate_series(1, 1000) as id,
        (ST_Dump(ST_GeneratePoints(ST_GeomFromText('POLYGON ((-180 90, 180 90, 180 -90, -180 -90, -180 90))', 4326), 1000))).geom;

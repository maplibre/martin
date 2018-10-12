CREATE TABLE points(gid SERIAL PRIMARY KEY, geom GEOMETRY(GEOMETRY, 4326));

INSERT INTO points
    SELECT
        generate_series(1, 10000) as id,
        (
            ST_DUMP(
                ST_GENERATEPOINTS(
                    ST_GEOMFROMTEXT('POLYGON ((-180 90, 180 90, 180 -90, -180 -90, -180 90))', 4326),
                    10000
                )
            )
        ).geom;

CREATE INDEX ON points USING GIST(geom);
CLUSTER points_geom_idx ON points;
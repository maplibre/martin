CREATE TABLE points3(gid SERIAL PRIMARY KEY, fld1 TEXT, fld2 TEXT, geom GEOMETRY(POINT, 4326));

INSERT INTO points3
    SELECT
        generate_series(1, 10000) as id,
        md5(random()::text) as fld1,
        md5(random()::text) as fld2,
        (
            ST_DUMP(
                ST_GENERATEPOINTS(
                    ST_GEOMFROMTEXT('POLYGON ((-180 90, 180 90, 180 -90, -180 -90, -180 90))', 4326),
                    10000
                )
            )
        ).geom;

CREATE INDEX ON points3 USING GIST(geom);
CLUSTER points3_geom_idx ON points3;

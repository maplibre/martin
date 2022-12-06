CREATE TABLE Points3(Gid SERIAL PRIMARY KEY, Fld1 TEXT, Fld2 TEXT, Geom GEOMETRY(POINT, 4326));

INSERT INTO Points3
    SELECT
        generate_series(1, 10000) as id,
        md5(random()::text) as Fld1,
        md5(random()::text) as Fld2,
        (
            ST_DUMP(
                ST_GENERATEPOINTS(
                    ST_GEOMFROMTEXT('POLYGON ((-180 90, 180 90, 180 -90, -180 -90, -180 90))', 4326),
                    10000
                )
            )
        ).Geom;

CREATE INDEX ON Points3 USING GIST(Geom);
CLUSTER Points3_geom_idx ON Points3;

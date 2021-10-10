CREATE TABLE points3857(gid SERIAL PRIMARY KEY, geom GEOMETRY(POINT, 3857));

INSERT INTO points3857
    SELECT
        generate_series(1, 10000) as id,
        (
            ST_DUMP(
                ST_GENERATEPOINTS(
                    ST_TRANSFORM(
                        ST_GEOMFROMTEXT('POLYGON ((-179 89, 179 89, 179 -89, -179 -89, -179 89))', 4326),
                        3857
                    ),
                    10000
                )
            )
        ).geom;

CREATE INDEX ON points3857 USING GIST(geom);
CLUSTER points3857_geom_idx ON points3857;
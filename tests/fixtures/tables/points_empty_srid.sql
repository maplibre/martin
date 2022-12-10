CREATE TABLE points_empty_srid(gid SERIAL PRIMARY KEY, geom GEOMETRY);

INSERT INTO points_empty_srid
    SELECT
        generate_series(1, 10000) as id,
        (
            ST_DUMP(
                ST_GENERATEPOINTS(
                    ST_TRANSFORM(
                        ST_GEOMFROMTEXT('POLYGON ((-179 89, 179 89, 179 -89, -179 -89, -179 89))', 4326),
                        900913
                    ),
                    10000
                )
            )
        ).geom;

CREATE INDEX ON points_empty_srid USING GIST(geom);
CLUSTER points_empty_srid_geom_idx ON points_empty_srid;

DROP TABLE IF EXISTS array_props;
CREATE TABLE array_props (
    feat_id int4 PRIMARY KEY,
    tags int4[],
    label text, -- noqa: RF04
    geom GEOMETRY (POINT, 4326)
);

INSERT INTO array_props VALUES (
    1, '{1,2,3}', 'one', GEOMFROMEWKT('SRID=4326;POINT(0 0)')
);
INSERT INTO array_props VALUES (
    2, null, 'two', GEOMFROMEWKT('SRID=4326;POINT(1 1)')
);

CREATE INDEX array_props_geom_idx ON array_props USING gist (geom);

DO $do$ BEGIN
    EXECUTE 'COMMENT ON TABLE array_props IS $tj$' || $$
    {
        "description": "An array column, which the row-per-feature MLT path cannot encode"
    }
    $$::json || '$tj$';
END $do$;

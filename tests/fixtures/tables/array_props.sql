DROP TABLE IF EXISTS array_props;
CREATE TABLE array_props (
    feat_id int4 PRIMARY KEY,
    tags int4[],
    label text, -- noqa: RF04
    doc jsonb,
    geom GEOMETRY (POINT, 4326)
);

INSERT INTO array_props VALUES (
    1,
    '{1,2,3}',
    'one',
    '{"rank": 1, "score": 1.5, "kind": "a", "deep": {"k": 1}, "list": [1, 2], "gone": null}',
    GEOMFROMEWKT('SRID=4326;POINT(0 0)')
);
INSERT INTO array_props VALUES (
    2,
    null,
    'two',
    '{"rank": -2, "score": 3, "label": "from the document"}',
    GEOMFROMEWKT('SRID=4326;POINT(1 1)')
);

CREATE INDEX array_props_geom_idx ON array_props USING gist (geom);

DO $do$ BEGIN
    EXECUTE 'COMMENT ON TABLE array_props IS $tj$' || $$
    {
        "description": "Array and jsonb columns, which ST_AsMVT writes as text and as one property per top-level key"
    }
    $$::json || '$tj$';
END $do$;

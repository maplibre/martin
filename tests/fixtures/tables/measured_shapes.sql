DROP TABLE IF EXISTS measured_shapes;
CREATE TABLE measured_shapes ( -- noqa: PRS
    feat_id int4 PRIMARY KEY,
    big int8,
    small int2,
    signed int4,
    flag bool,
    single float4,
    double float8,
    label text,
    code varchar(8),
    never_set int4,
    geom GEOMETRY (GEOMETRYM, 4326)
);

INSERT INTO measured_shapes VALUES (
    1, 9000000000, 3, -7, true, 1.5, 2.25, 'one', 'v1', null,
    GEOMFROMEWKT('SRID=4326;POINTM(0 0 11)')
);
INSERT INTO measured_shapes VALUES (
    2, null, null, 5, false, null, null, null, null, null,
    GEOMFROMEWKT('SRID=4326;LINESTRINGM(1 1 1, 2 2 2, 3 3 3)')
);
INSERT INTO measured_shapes VALUES (
    3, 1, -3, 0, null, -0.5, -0.25, 'three', 'v3', null,
    GEOMFROMEWKT('SRID=4326;POLYGONM((0 0 1, 4 0 2, 4 4 3, 0 4 4, 0 0 5))')
);
INSERT INTO measured_shapes VALUES (
    4, 2, 2, 2, true, 0.0, 0.0, '', '', null,
    GEOMFROMEWKT('SRID=4326;MULTIPOINTM((5 5 1), (6 6 2))')
);
INSERT INTO measured_shapes VALUES (
    5, 3, 3, 3, false, 3.5, 3.5, 'five', 'v5', null,
    GEOMFROMEWKT('SRID=4326;MULTIPOINTM((7 7 9))')
);
INSERT INTO measured_shapes VALUES (
    6, 4, 4, 4, true, 4.5, 4.5, 'six', 'v6', null,
    GEOMFROMEWKT(
        'SRID=4326;MULTIPOLYGONM(((8 8 1, 9 8 2, 9 9 3, 8 9 4, 8 8 5)))'
    )
);

CREATE INDEX measured_shapes_geom_idx ON measured_shapes USING gist (geom);

DO $do$ BEGIN
    EXECUTE 'COMMENT ON TABLE measured_shapes IS $tj$' || $$
    {
        "description": "Geometries carrying an M ordinate, in every property type martin serves"
    }
    $$::json || '$tj$';
END $do$;

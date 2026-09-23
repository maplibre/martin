DROP TABLE IF EXISTS dimensioned_shapes;
CREATE TABLE dimensioned_shapes ( -- noqa: PRS
    feat_id int4 PRIMARY KEY,
    dims text NOT NULL,
    kind text NOT NULL,
    geom geometry NOT NULL CHECK (ST_SRID(geom) = 4326)
);

INSERT INTO dimensioned_shapes VALUES
(1, 'xy', 'point', GEOMFROMEWKT('SRID=4326;POINT(0 0)')),
(
    2, 'xy', 'linestring',
    GEOMFROMEWKT('SRID=4326;LINESTRING(10 0, 20 10, 30 0)')
),
(
    3, 'xy', 'polygon',
    GEOMFROMEWKT(
        'SRID=4326;POLYGON((40 0, 60 0, 60 20, 40 20, 40 0), '
        || '(45 5, 45 15, 55 15, 55 5, 45 5))'
    )
),
(
    4, 'xy', 'multipoint',
    GEOMFROMEWKT('SRID=4326;MULTIPOINT((70 0), (80 10))')
),
(
    5, 'xy', 'multilinestring',
    GEOMFROMEWKT('SRID=4326;MULTILINESTRING((90 0, 100 10), (90 10, 100 0))')
),
(
    6, 'xy', 'multipolygon',
    GEOMFROMEWKT(
        'SRID=4326;MULTIPOLYGON(((110 0, 120 0, 120 10, 110 10, 110 0)), '
        || '((125 0, 135 0, 135 10, 125 10, 125 0)))'
    )
),
(
    7, 'xy', 'geometrycollection',
    GEOMFROMEWKT(
        'SRID=4326;GEOMETRYCOLLECTION(POINT(140 0), '
        || 'LINESTRING(140 10, 150 20))'
    )
),
(11, 'xyz', 'point', GEOMFROMEWKT('SRID=4326;POINTZ(0 0 100)')),
(
    12, 'xyz', 'linestring',
    GEOMFROMEWKT('SRID=4326;LINESTRINGZ(10 0 101, 20 10 102, 30 0 103)')
),
(
    13, 'xyz', 'polygon',
    GEOMFROMEWKT(
        'SRID=4326;POLYGONZ((40 0 101, 60 0 102, 60 20 103, 40 20 104, '
        || '40 0 101), (45 5 105, 45 15 106, 55 15 107, 55 5 108, 45 5 105))'
    )
),
(
    14, 'xyz', 'multipoint',
    GEOMFROMEWKT('SRID=4326;MULTIPOINTZ((70 0 101), (80 10 102))')
),
(
    15, 'xyz', 'multilinestring',
    GEOMFROMEWKT(
        'SRID=4326;MULTILINESTRINGZ((90 0 101, 100 10 102), '
        || '(90 10 103, 100 0 104))'
    )
),
(
    16, 'xyz', 'multipolygon',
    GEOMFROMEWKT(
        'SRID=4326;MULTIPOLYGONZ(((110 0 101, 120 0 102, 120 10 103, '
        || '110 10 104, 110 0 101)), ((125 0 105, 135 0 106, 135 10 107, '
        || '125 10 108, 125 0 105)))'
    )
),
(
    17, 'xyz', 'geometrycollection',
    GEOMFROMEWKT(
        'SRID=4326;GEOMETRYCOLLECTIONZ(POINTZ(140 0 101), '
        || 'LINESTRINGZ(140 10 102, 150 20 103))'
    )
),
(21, 'xym', 'point', GEOMFROMEWKT('SRID=4326;POINTM(0 0 1)')),
(
    22, 'xym', 'linestring',
    GEOMFROMEWKT('SRID=4326;LINESTRINGM(10 0 1, 20 10 2, 30 0 3)')
),
(
    23, 'xym', 'polygon',
    GEOMFROMEWKT(
        'SRID=4326;POLYGONM((40 0 1, 60 0 2, 60 20 3, 40 20 4, 40 0 1), '
        || '(45 5 5, 45 15 6, 55 15 7, 55 5 8, 45 5 5))'
    )
),
(
    24, 'xym', 'multipoint',
    GEOMFROMEWKT('SRID=4326;MULTIPOINTM((70 0 1), (80 10 2))')
),
(
    25, 'xym', 'multilinestring',
    GEOMFROMEWKT(
        'SRID=4326;MULTILINESTRINGM((90 0 1, 100 10 2), (90 10 3, 100 0 4))'
    )
),
(
    26, 'xym', 'multipolygon',
    GEOMFROMEWKT(
        'SRID=4326;MULTIPOLYGONM(((110 0 1, 120 0 2, 120 10 3, 110 10 4, '
        || '110 0 1)), ((125 0 5, 135 0 6, 135 10 7, 125 10 8, 125 0 5)))'
    )
),
(
    27, 'xym', 'geometrycollection',
    GEOMFROMEWKT(
        'SRID=4326;GEOMETRYCOLLECTIONM(POINTM(140 0 1), '
        || 'LINESTRINGM(140 10 2, 150 20 3))'
    )
),
(31, 'xyzm', 'point', GEOMFROMEWKT('SRID=4326;POINTZM(0 0 100 1)')),
(
    32, 'xyzm', 'linestring',
    GEOMFROMEWKT(
        'SRID=4326;LINESTRINGZM(10 0 101 1, 20 10 102 2, 30 0 103 3)'
    )
),
(
    33, 'xyzm', 'polygon',
    GEOMFROMEWKT(
        'SRID=4326;POLYGONZM((40 0 101 1, 60 0 102 2, 60 20 103 3, '
        || '40 20 104 4, 40 0 101 1), (45 5 105 5, 45 15 106 6, '
        || '55 15 107 7, 55 5 108 8, 45 5 105 5))'
    )
),
(
    34, 'xyzm', 'multipoint',
    GEOMFROMEWKT('SRID=4326;MULTIPOINTZM((70 0 101 1), (80 10 102 2))')
),
(
    35, 'xyzm', 'multilinestring',
    GEOMFROMEWKT(
        'SRID=4326;MULTILINESTRINGZM((90 0 101 1, 100 10 102 2), '
        || '(90 10 103 3, 100 0 104 4))'
    )
),
(
    36, 'xyzm', 'multipolygon',
    GEOMFROMEWKT(
        'SRID=4326;MULTIPOLYGONZM(((110 0 101 1, 120 0 102 2, '
        || '120 10 103 3, 110 10 104 4, 110 0 101 1)), '
        || '((125 0 105 5, 135 0 106 6, 135 10 107 7, 125 10 108 8, '
        || '125 0 105 5)))'
    )
),
(
    37, 'xyzm', 'geometrycollection',
    GEOMFROMEWKT(
        'SRID=4326;GEOMETRYCOLLECTIONZM(POINTZM(140 0 101 1), '
        || 'LINESTRINGZM(140 10 102 2, 150 20 103 3))'
    )
);

CREATE INDEX dimensioned_shapes_geom_idx ON dimensioned_shapes USING gist (geom);

DO $do$ BEGIN
    EXECUTE 'COMMENT ON TABLE dimensioned_shapes IS $tj$' || $$
    {
        "description": "Every geometry type, in XY, XYZ, XYM and XYZM"
    }
    $$::json || '$tj$';
END $do$;

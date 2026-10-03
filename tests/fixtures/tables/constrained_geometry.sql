DROP TABLE IF EXISTS constrained_geometry;
CREATE TABLE constrained_geometry (
    gid int4 PRIMARY KEY,
    geom geometry,
    CONSTRAINT enforce_srid_geom CHECK (ST_SRID(geom) = 4326),
    CONSTRAINT enforce_geotype_geom CHECK (GEOMETRYTYPE(geom) = 'POINT'::text OR geom IS NULL),
    CONSTRAINT enforce_dims_geom CHECK (ST_NDIMS(geom) = 2)
);

INSERT INTO constrained_geometry VALUES (1, GEOMFROMEWKT('SRID=4326;POINT(0 0)'));
INSERT INTO constrained_geometry VALUES (2, GEOMFROMEWKT('SRID=4326;POINT(1 1)'));

CREATE INDEX constrained_geometry_geom_idx ON constrained_geometry USING gist (geom);

DO $do$ BEGIN
    EXECUTE 'COMMENT ON TABLE constrained_geometry IS $tj$' || $$
    {
        "description": "An untyped geometry column whose SRID and type come from CHECK constraints"
    }
    $$::json || '$tj$';
END $do$;

DROP TABLE IF EXISTS domain_props;
DROP DOMAIN IF EXISTS small_count;
DROP DOMAIN IF EXISTS positive_count;
CREATE DOMAIN positive_count AS int4 CHECK (value > 0);
CREATE DOMAIN small_count AS positive_count CHECK (value < 100);
CREATE TABLE domain_props (
    feat_id int4 PRIMARY KEY,
    visitors positive_count,
    floors small_count,
    geom GEOMETRY (POINT, 4326)
);

INSERT INTO domain_props VALUES (1, 1200, 3, GEOMFROMEWKT('SRID=4326;POINT(0 0)'));
INSERT INTO domain_props VALUES (2, 45, 12, GEOMFROMEWKT('SRID=4326;POINT(1 1)'));

CREATE INDEX domain_props_geom_idx ON domain_props USING gist (geom);

DO $do$ BEGIN
    EXECUTE 'COMMENT ON TABLE domain_props IS $tj$' || $$
    {
        "description": "A domain column and a domain over a domain, which a query returns as the integer under them"
    }
    $$::json || '$tj$';
END $do$;

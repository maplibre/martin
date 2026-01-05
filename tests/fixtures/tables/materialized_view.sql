CREATE TABLE mat_view_src (
    id serial PRIMARY KEY,
    txt text
);

INSERT INTO mat_view_src (txt) VALUES
    ('POINT(-122.4194 37.7749)'),
    ('POINT(-73.935242 40.730610)');

CREATE MATERIALIZED VIEW mat_view AS
SELECT
    id,
    ST_GEOMFROMTEXT(txt, 4326) geom
FROM mat_view_src;

DO $do$ BEGIN
    EXECUTE 'COMMENT ON MATERIALIZED VIEW mat_view IS $tj$' || $$
    {
      "description": "materialized view comment"
    }
    $$::json || '$tj$';
END $do$;

CREATE TABLE fixtures_comments (
    id serial PRIMARY KEY,
    txt text,
    geom GEOMETRY (POINT, 4326)
);

INSERT INTO fixtures_comments (txt, geom) VALUES
('a', ST_GEOMFROMTEXT('POINT(-122.4194 37.7749)', 4326)),
('b', ST_GEOMFROMTEXT('POINT(-73.935242 40.730610)', 4326));

CREATE MATERIALIZED VIEW fixtures_mv_comments AS
SELECT id, txt, geom
FROM fixtures_comments;

COMMENT ON MATERIALIZED VIEW fixtures_mv_comments IS 'fixture: materialized view comments';

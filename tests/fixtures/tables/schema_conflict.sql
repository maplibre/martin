CREATE SCHEMA IF NOT EXISTS schema_a;
CREATE SCHEMA IF NOT EXISTS schema_b;

CREATE TABLE schema_a.table_name_existing_two_schemas
(
    a_id SERIAL PRIMARY KEY,
    a_name TEXT,
    a_geom GEOMETRY (POINT, 4326)
);

CREATE TABLE schema_b.table_name_existing_two_schemas
(
    b_id SERIAL PRIMARY KEY,
    b_info TEXT,
    b_geom GEOMETRY (POLYGON, 4326)
);

CREATE VIEW schema_a.view_name_existing_two_schemas AS
SELECT
    a_id,
    a_geom,
    'view_' || a_name AS a_name
FROM schema_a.table_name_existing_two_schemas;

CREATE VIEW schema_b.view_name_existing_two_schemas AS
SELECT
    b_id,
    b_geom,
    'view_' || b_info AS b_info
FROM schema_b.table_name_existing_two_schemas;

INSERT INTO schema_a.table_name_existing_two_schemas (a_name, a_geom)
VALUES ('point_1', '0101000020E6100000EC3A2806EDDA61401C2041E87DDA2740'),
('point_2', '0101000020E61000005DDA9603E9DA614070BB4C49D0DA2740');

INSERT INTO schema_b.table_name_existing_two_schemas (b_info, b_geom)
VALUES ('polygon_1', GEOMFROMEWKT('SRID=4326;POLYGON((30 10, 40 40, 20 40, 10 20, 30 10))'));

CREATE INDEX ON schema_a.table_name_existing_two_schemas USING gist (a_geom);
CLUSTER table_name_existing_two_schemas_a_geom_idx ON schema_a.table_name_existing_two_schemas;
CREATE INDEX ON schema_b.table_name_existing_two_schemas USING gist (b_geom);
CLUSTER table_name_existing_two_schemas_b_geom_idx ON schema_b.table_name_existing_two_schemas;

-- instead of tables or views, we can also mix them
CREATE TABLE schema_a.table_and_view_two_schemas (
    a_id SERIAL PRIMARY KEY,
    a_info TEXT,
    a_geom GEOMETRY
);

INSERT INTO schema_a.table_and_view_two_schemas (a_info, a_geom)
VALUES
('point_1', GEOMFROMEWKT('SRID=4326;POINT(1 1)')),
('point_2', GEOMFROMEWKT('SRID=4326;POINT(2 2)'));

CREATE INDEX ON schema_a.table_and_view_two_schemas USING gist (a_geom);
CLUSTER table_and_view_two_schemas_a_geom_idx ON schema_a.table_and_view_two_schemas;

CREATE VIEW schema_b.table_and_view_two_schemas AS SELECT
    a_id AS b_id,
    a_info AS b_info,
    a_geom AS b_geom
FROM schema_a.table_and_view_two_schemas;

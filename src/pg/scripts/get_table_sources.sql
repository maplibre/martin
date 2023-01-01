WITH columns AS (
  SELECT
    ns.nspname AS table_schema,
    class.relname AS table_name,
    attr.attname AS column_name,
    trim(leading '_' from tp.typname) AS type_name
  FROM pg_attribute attr
    JOIN pg_catalog.pg_class AS class ON class.oid = attr.attrelid
    JOIN pg_catalog.pg_namespace AS ns ON ns.oid = class.relnamespace
    JOIN pg_catalog.pg_type AS tp ON tp.oid = attr.atttypid
  WHERE NOT attr.attisdropped AND attr.attnum > 0)
SELECT
  f_table_schema AS schema, f_table_name AS name, f_geometry_column AS geom, srid, type,
    COALESCE(
      jsonb_object_agg(columns.column_name, columns.type_name) FILTER (WHERE columns.column_name IS NOT NULL),
      '{}'::jsonb
    ) as properties
FROM geometry_columns
LEFT JOIN columns ON
  geometry_columns.f_table_schema = columns.table_schema AND
  geometry_columns.f_table_name = columns.table_name AND
  geometry_columns.f_geometry_column != columns.column_name
GROUP BY f_table_schema, f_table_name, f_geometry_column, srid, type;

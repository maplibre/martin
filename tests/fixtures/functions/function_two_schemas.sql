DROP FUNCTION IF EXISTS schema_a.function_two_schemas(integer, integer, integer);
DROP FUNCTION IF EXISTS schema_b.function_two_schemas(integer, integer, integer);

-- The same name in two schemas, each with its own comment.
CREATE OR REPLACE FUNCTION schema_a.function_two_schemas(
    z integer, x integer, y integer
) RETURNS bytea AS $$
BEGIN
  RETURN public.function_zxy(z, x, y);
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;

CREATE OR REPLACE FUNCTION schema_b.function_two_schemas(
    z integer, x integer, y integer
) RETURNS bytea AS $$
BEGIN
  RETURN public.function_zxy(z, x, y);
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;

DO $do$ BEGIN
    EXECUTE 'COMMENT ON FUNCTION schema_a.function_two_schemas (INT4, INT4, INT4) IS $tj$' || $$
    {
      "description": "the schema_a comment"
    }
    $$::json || '$tj$';
    EXECUTE 'COMMENT ON FUNCTION schema_b.function_two_schemas (INT4, INT4, INT4) IS $tj$' || $$
    {
      "description": "the schema_b comment"
    }
    $$::json || '$tj$';
END $do$;

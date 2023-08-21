DROP FUNCTION IF EXISTS public.function_null_row2;

CREATE OR REPLACE FUNCTION public.function_null_row2(z integer, x integer, y integer)
RETURNS TABLE(mvt bytea, key text) AS $$
  SELECT NULL::bytea, NULL::text WHERE FALSE;
$$ LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE;

DO $do$ BEGIN
    EXECUTE 'COMMENT ON FUNCTION public.function_null_row2 (INT4, INT4, INT4) IS $tj$' || $$
    {
      "tilejson": "3.0.0",
      "tiles": [],
      "minzoom": 0,
      "maxzoom": 18,
      "bounds": [
          -180,
          -85,
          180,
          85
      ],
      "vector_layers": []
    }
    $$::json || '$tj$';
END $do$;

DROP FUNCTION IF EXISTS public.function_null_row;

CREATE OR REPLACE FUNCTION public.function_null_row(z integer, x integer, y integer)
RETURNS TABLE(mvt bytea, key text) AS $$
  SELECT NULL::bytea, NULL::text
$$ LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE;

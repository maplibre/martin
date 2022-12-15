DROP FUNCTION IF EXISTS public.function_null_row2;

CREATE OR REPLACE FUNCTION public.function_null_row2(z integer, x integer, y integer)
RETURNS TABLE(mvt bytea, key text) AS $$
  SELECT NULL::bytea, NULL::text WHERE FALSE;
$$ LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE;

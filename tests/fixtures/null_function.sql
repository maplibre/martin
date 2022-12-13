DROP FUNCTION IF EXISTS public.null_function;
CREATE OR REPLACE FUNCTION public.null_function(z integer, x integer, y integer, query_params json) RETURNS bytea AS $$
BEGIN
  RETURN null;
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;

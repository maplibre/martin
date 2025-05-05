DROP FUNCTION IF EXISTS public.function_null;

CREATE OR REPLACE FUNCTION public.function_null(
    z integer, x integer, y integer
) RETURNS bytea AS $$
BEGIN
    RETURN null;
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;

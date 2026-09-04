DROP FUNCTION IF EXISTS public.function_dup(integer, integer, integer);
DROP FUNCTION IF EXISTS public.function_dup(integer, integer, integer, json);

-- One name with two signatures, each handing off to a function whose layer name tells them apart.
CREATE OR REPLACE FUNCTION public.function_dup(
    z integer, x integer, y integer
) RETURNS bytea AS $$
BEGIN
  RETURN public.function_zxy(z, x, y);
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;

CREATE OR REPLACE FUNCTION public.function_dup(
    z integer, x integer, y integer, query json
) RETURNS bytea AS $$
BEGIN
  RETURN public.function_zxy_query(z, x, y, query);
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;

DO $do$ BEGIN
    EXECUTE 'COMMENT ON FUNCTION public.function_dup (INT4, INT4, INT4, JSON) IS $tj$' || $$
    {
      "description": "the variant that takes a query"
    }
    $$::json || '$tj$';
END $do$;

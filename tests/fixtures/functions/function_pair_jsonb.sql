DROP FUNCTION IF EXISTS public.function_pair_jsonb(integer, integer, integer);
DROP FUNCTION IF EXISTS public.function_pair_jsonb(integer, integer, integer, jsonb);

-- A queryless variant next to a jsonb one.
CREATE OR REPLACE FUNCTION public.function_pair_jsonb(
    z integer, x integer, y integer
) RETURNS bytea AS $$
BEGIN
  RETURN public.function_zxy(z, x, y);
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;

CREATE OR REPLACE FUNCTION public.function_pair_jsonb(
    z integer, x integer, y integer, query jsonb
) RETURNS bytea AS $$
BEGIN
  RETURN public.function_zxy_query_jsonb(z, x, y, query);
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;

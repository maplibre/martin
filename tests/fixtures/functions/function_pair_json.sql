DROP FUNCTION IF EXISTS public.function_pair_json(integer, integer, integer);
DROP FUNCTION IF EXISTS public.function_pair_json(integer, integer, integer, json);

-- A queryless variant next to a json one.
CREATE OR REPLACE FUNCTION public.function_pair_json(
    z integer, x integer, y integer
) RETURNS bytea AS $$
BEGIN
  RETURN public.function_zxy(z, x, y);
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;

CREATE OR REPLACE FUNCTION public.function_pair_json(
    z integer, x integer, y integer, query json
) RETURNS bytea AS $$
BEGIN
  RETURN public.function_zxy_query(z, x, y, query);
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;

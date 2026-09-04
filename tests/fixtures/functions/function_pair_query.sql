DROP FUNCTION IF EXISTS public.function_pair_query(integer, integer, integer, json);
DROP FUNCTION IF EXISTS public.function_pair_query(integer, integer, integer, jsonb);

-- A json variant next to a jsonb one, with no queryless variant.
CREATE OR REPLACE FUNCTION public.function_pair_query(
    z integer, x integer, y integer, query json
) RETURNS bytea AS $$
BEGIN
  RETURN public.function_zxy_query(z, x, y, query);
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;

CREATE OR REPLACE FUNCTION public.function_pair_query(
    z integer, x integer, y integer, query jsonb
) RETURNS bytea AS $$
BEGIN
  RETURN public.function_zxy_query_jsonb(z, x, y, query);
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;

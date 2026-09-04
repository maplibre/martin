DROP FUNCTION IF EXISTS public.function_dup(integer, integer, integer);
DROP FUNCTION IF EXISTS public.function_dup(integer, integer, integer, json);
DROP FUNCTION IF EXISTS public.function_dup(integer, integer, integer, jsonb);

-- One name with three signatures and three comments, each handing off to a function whose layer name tells them apart.
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

CREATE OR REPLACE FUNCTION public.function_dup(
    z integer, x integer, y integer, query jsonb
) RETURNS bytea AS $$
BEGIN
  RETURN public.function_zxy_query_jsonb(z, x, y, query);
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;

DO $do$ BEGIN
    EXECUTE 'COMMENT ON FUNCTION public.function_dup (INT4, INT4, INT4) IS $tj$' || $$
    {
      "description": "the queryless variant",
      "attribution": "from the queryless comment"
    }
    $$::json || '$tj$';
    EXECUTE 'COMMENT ON FUNCTION public.function_dup (INT4, INT4, INT4, JSON) IS $tj$' || $$
    {
      "description": "the json variant"
    }
    $$::json || '$tj$';
    EXECUTE 'COMMENT ON FUNCTION public.function_dup (INT4, INT4, INT4, JSONB) IS $tj$' || $$
    {
      "description": "the jsonb variant"
    }
    $$::json || '$tj$';
END $do$;

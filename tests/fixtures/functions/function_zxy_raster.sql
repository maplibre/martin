DROP FUNCTION IF EXISTS public.function_zxy_raster;

CREATE OR REPLACE FUNCTION public.function_zxy_raster(
    z integer, x integer, y integer
) RETURNS bytea AS $$
  -- Returns empty bytea for testing purposes.
  -- In production this would return real raster tile data (e.g. PNG, JPEG).
  SELECT '\x'::bytea
$$ LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE;

DO $do$ BEGIN
    EXECUTE 'COMMENT ON FUNCTION public.function_zxy_raster IS $tj$' || $$
    {
        "description": "a raster tile function source",
        "content_type": "image/png"
    }
    $$::json || '$tj$';
END $do$;

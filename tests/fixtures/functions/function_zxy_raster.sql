DROP FUNCTION IF EXISTS public.function_zxy_raster;

CREATE OR REPLACE FUNCTION public.function_zxy_raster(
    z integer, x integer, y integer
) RETURNS bytea AS $$
  -- Returns a minimal 1x1 white PNG for testing purposes.
  SELECT '\x89504e470d0a1a0a0000000d4948445200000001000000010802000000907753de0000000c4944415478da63f8ffff3f0005fe02fe331295140000000049454e44ae426082'::bytea
$$ LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE;

DO $do$ BEGIN
    EXECUTE 'COMMENT ON FUNCTION public.function_zxy_raster IS $tj$' || $$
    {
        "description": "a raster tile function source",
        "content_type": "image/png"
    }
    $$::json || '$tj$';
END $do$;

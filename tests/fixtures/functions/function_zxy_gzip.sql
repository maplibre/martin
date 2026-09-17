DROP FUNCTION IF EXISTS public.function_zxy_gzip;

CREATE OR REPLACE FUNCTION public.function_zxy_gzip(
    z integer, x integer, y integer
) RETURNS bytea AS $$
  -- Returns the tile function_zxy produces for 6/57/29, gzip-compressed, for every coordinate.
  SELECT '\x1f8b08000000000002ff93ea60e4122e284dcac94cd64b2bcd4b2ec9cccf8bafaaa814e294605462e59c23b9cc1cca9c25b9c21c2e8a9db9cc1caf3614268a5a8d06850a2600b435fffc8b000000'::bytea
$$ LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE;

DO $do$ BEGIN
    EXECUTE 'COMMENT ON FUNCTION public.function_zxy_gzip IS $tj$' || $$
    {
        "description": "a function source returning gzip-compressed tiles"
    }
    $$::json || '$tj$';
END $do$;

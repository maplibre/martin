DROP FUNCTION IF EXISTS public.function_zxy2;

CREATE OR REPLACE FUNCTION public.function_zxy2(
    z integer, x integer, y integer
) RETURNS bytea AS $$
  SELECT ST_AsMVT(tile, 'public.function_zxy2', 4096, 'geom') FROM (
    SELECT
      ST_AsMVTGeom(ST_Transform(ST_CurveToLine(geom), 3857), ST_TileEnvelope(z, x, y), 4096, 64, true) AS geom
    FROM public.table_source
    WHERE geom && ST_Transform(ST_TileEnvelope(z, x, y), 4326)
  ) as tile WHERE geom IS NOT NULL
$$ LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE;

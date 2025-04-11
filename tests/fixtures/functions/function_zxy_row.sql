-- Uses mixed case names but without double quotes

DROP FUNCTION IF EXISTS public.function_zxy_row;

CREATE OR REPLACE FUNCTION public.function_zxy_ROW(
    Z integer, x integer, y integer
)
RETURNS TABLE (mvt bytea) AS $$
  SELECT ST_AsMVT(tile, 'public.function_zxy_ROW', 4096, 'geom') as MVT FROM (
    SELECT
      ST_AsMVTGeom(ST_Transform(ST_CurveToLine(geom), 3857), ST_TileEnvelope(Z, x, y), 4096, 64, true) AS geom
    FROM public.table_source
    WHERE geom && ST_Transform(ST_TileEnvelope(Z, x, y), 4326)
  ) as tile WHERE geom IS NOT NULL
$$ LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE;

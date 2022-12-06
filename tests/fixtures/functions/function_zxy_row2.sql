DROP FUNCTION IF EXISTS public.Function_Zxy_row2;

CREATE OR REPLACE FUNCTION public.Function_Zxy_row2(Z integer, x integer, y integer)
RETURNS TABLE(mvt bytea, key text) AS $$
  SELECT mvt, md5(mvt) as key FROM (
    SELECT ST_AsMVT(tile, 'public.Function_Zxy_row2', 4096, 'geom') as mvt FROM (
      SELECT
        ST_AsMVTGeom(
            ST_Transform(ST_CurveToLine(geom), 3857),
            ST_TileEnvelope(Z, x, y),
            4096, 64, true) AS geom
      FROM public.table_source
      WHERE geom && ST_Transform(ST_TileEnvelope(Z, x, y), 4326)
  ) as tile WHERE geom IS NOT NULL) src
$$ LANGUAGE SQL IMMUTABLE STRICT PARALLEL SAFE;

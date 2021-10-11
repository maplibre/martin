DROP FUNCTION IF EXISTS public.function_source;
CREATE OR REPLACE FUNCTION public.function_source(z integer, x integer, y integer, query_params json) RETURNS bytea AS $$
DECLARE
  mvt bytea;
BEGIN
  RAISE NOTICE 'query_params: %', query_params;

  SELECT INTO mvt ST_AsMVT(tile, 'public.function_source', 4096, 'geom') FROM (
    SELECT
      ST_AsMVTGeom(ST_Transform(ST_CurveToLine(geom), 3857), TileBBox(z, x, y, 3857), 4096, 64, true) AS geom
    FROM public.table_source
    WHERE geom && TileBBox(z, x, y, 4326)
  ) as tile WHERE geom IS NOT NULL;

  RETURN mvt;
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;
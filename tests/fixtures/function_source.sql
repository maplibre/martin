DROP FUNCTION IF EXISTS public.function_source;
CREATE OR REPLACE FUNCTION public.function_source(z integer, x integer, y integer, query_params json) RETURNS bytea AS $$
DECLARE
  bounds geometry;
  mvt bytea;
BEGIN
  SELECT INTO bounds TileBBox(z, x, y, 3857);
  
  SELECT INTO mvt ST_AsMVT(tile, 'public.function_source', 4096, 'geom') FROM (
    SELECT
      ST_AsMVTGeom(geom, bounds, 4096, 64, true) AS geom
    FROM public.table_source
    WHERE geom && bounds
  ) as tile WHERE geom IS NOT NULL;

  RETURN mvt;
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;
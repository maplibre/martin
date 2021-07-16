DROP FUNCTION IF EXISTS public.function_source_query_params;
CREATE OR REPLACE FUNCTION public.function_source_query_params(z integer, x integer, y integer, query_params json) RETURNS bytea AS $$
DECLARE
  mvt bytea;
BEGIN
  RAISE DEBUG 'query_params: %', query_params;

  IF (query_params->>'token')::varchar IS NULL THEN
    RAISE EXCEPTION 'the `token` json parameter does not exist in `query_params`';
  END IF;

  SELECT INTO mvt ST_AsMVT(tile, 'public.function_source_query_params', 4096, 'geom') FROM (
    SELECT
      ST_AsMVTGeom(ST_Transform(geom, 3857), TileBBox(z, x, y, 3857), 4096, 64, true) AS geom
    FROM public.table_source
    WHERE geom && TileBBox(z, x, y, 4326)
  ) as tile WHERE geom IS NOT NULL;

  RETURN mvt;
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;
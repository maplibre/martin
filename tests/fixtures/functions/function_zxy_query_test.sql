DROP FUNCTION IF EXISTS public.function_zxy_query_test;
CREATE OR REPLACE FUNCTION public.function_zxy_query_test(z integer, x integer, y integer, query_params json) RETURNS bytea AS $$
DECLARE
  mvt bytea;
BEGIN
  RAISE DEBUG 'query_params: %', query_params;

  IF (query_params->>'token')::varchar IS NULL THEN
    RAISE EXCEPTION 'the `token` json parameter does not exist in `query_params`';
  END IF;

  SELECT INTO mvt ST_AsMVT(tile, 'public.function_zxy_query_test', 4096, 'geom') FROM (
    SELECT
      ST_AsMVTGeom(ST_Transform(ST_CurveToLine(geom), 3857), ST_TileEnvelope(z, x, y), 4096, 64, true) AS geom
    FROM public.table_source
    WHERE geom && ST_Transform(ST_TileEnvelope(z, x, y), 4326)
  ) as tile WHERE geom IS NOT NULL;

  RETURN mvt;
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;

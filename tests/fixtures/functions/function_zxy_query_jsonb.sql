DROP FUNCTION IF EXISTS public.function_zxy_query_jsonb;

CREATE OR REPLACE FUNCTION public.function_zxy_query_jsonb(z integer, x integer, y integer, query jsonb) RETURNS bytea AS $$
DECLARE
  mvt bytea;
BEGIN
  RAISE NOTICE 'query: %', query;

  SELECT INTO mvt ST_AsMVT(tile, 'public.function_zxy_query_jsonb', 4096, 'geom') FROM (
    SELECT
      ST_AsMVTGeom(ST_Transform(ST_CurveToLine(geom), 3857), ST_TileEnvelope(z, x, y), 4096, 64, true) AS geom
    FROM public.table_source
    WHERE geom && ST_Transform(ST_TileEnvelope(z, x, y), 4326)
  ) as tile WHERE geom IS NOT NULL;

  RETURN mvt;
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;

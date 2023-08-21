DROP FUNCTION IF EXISTS public.function_zxy_query;

CREATE OR REPLACE FUNCTION public.function_zxy_query(z integer, x integer, y integer, query json) RETURNS bytea AS $$
DECLARE
  mvt bytea;
BEGIN
  RAISE NOTICE 'query: %', query;

  SELECT INTO mvt ST_AsMVT(tile, 'public.function_zxy_query', 4096, 'geom') FROM (
    SELECT
      ST_AsMVTGeom(ST_Transform(ST_CurveToLine(geom), 3857), ST_TileEnvelope(z, x, y), 4096, 64, true) AS geom
    FROM public.table_source
    WHERE geom && ST_Transform(ST_TileEnvelope(z, x, y), 4326)
  ) as tile WHERE geom IS NOT NULL;

  RETURN mvt;
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;

DO $do$ BEGIN
    EXECUTE 'COMMENT ON FUNCTION public.function_zxy_query (INT4, INT4, INT4, JSON) IS $tj$' || $$
    {
      "tilejson": "3.0.0",
      "tiles": [],
      "minzoom": 0,
      "maxzoom": 18,
      "bounds": [
        -180,
        -85,
        180,
        85
      ],
      "vector_layers": [
          {
            "id": "public.function_zxy_query",
            "fields": {
              "geom": ""
            }
          }
      ]
    }
    $$::json || '$tj$';
END $do$;

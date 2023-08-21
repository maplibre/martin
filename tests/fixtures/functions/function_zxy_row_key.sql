DROP FUNCTION IF EXISTS public.function_zxy_row_key;

CREATE OR REPLACE FUNCTION public.function_zxy_row_key(z integer, x integer, y integer)
RETURNS TABLE(mvt bytea, key text) AS $$
  SELECT mvt, md5(mvt) as key FROM (
      SELECT ST_AsMVT(tile, 'public.function_zxy_row_key', 4096, 'geom') as mvt FROM (
        SELECT
          ST_AsMVTGeom(ST_Transform(ST_CurveToLine(geom), 3857), ST_TileEnvelope(z, x, y), 4096, 64, true) AS geom
        FROM public.table_source
        WHERE geom && ST_Transform(ST_TileEnvelope(z, x, y), 4326)
      ) as tile WHERE geom IS NOT NULL) src
$$ LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE;

DO $do$ BEGIN
    EXECUTE 'COMMENT ON FUNCTION public.function_zxy_row_key (INT4, INT4, INT4) IS $tj$' || $$
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
            "id": "public.function_zxy_row_key",
            "fields": {
                "geom": ""
            }
        }
      ]
    }
    $$::json || '$tj$';
END $do$;

-- Uses mixed case names but without double quotes

DROP FUNCTION IF EXISTS public.function_zxy_ROW;

CREATE OR REPLACE FUNCTION public.function_zxy_ROW(Z integer, x integer, y integer)
RETURNS TABLE(MVT bytea) AS $$
  SELECT ST_AsMVT(tile, 'public.function_zxy_ROW', 4096, 'geom') as MVT FROM (
    SELECT
      ST_AsMVTGeom(ST_Transform(ST_CurveToLine(geom), 3857), ST_TileEnvelope(Z, x, y), 4096, 64, true) AS geom
    FROM public.table_source
    WHERE geom && ST_Transform(ST_TileEnvelope(Z, x, y), 4326)
  ) as tile WHERE geom IS NOT NULL
$$ LANGUAGE SQL IMMUTABLE STRICT PARALLEL SAFE;

DO $do$ BEGIN
    EXECUTE 'COMMENT ON FUNCTION public.function_zxy_ROW (INT4, INT4, INT4) IS $tj$' || $$
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
            "id": "public.function_zxy_ROW",
            "fields": {
                "geom": ""
            }
        }
    ]
}
    $$::json || '$tj$';
END $do$;

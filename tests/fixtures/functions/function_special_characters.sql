-- noqa: disable=RF05
DROP FUNCTION IF EXISTS public."""function.withweired$*;_ characters";

CREATE OR REPLACE FUNCTION public."""function.withweired$*;_ characters"(
    "Z" integer, x integer, y integer
)
RETURNS TABLE ("mVt" bytea, key text) AS $$
  SELECT mvt, md5(mvt) as key FROM (
    SELECT ST_AsMVT(tile, 'public."function.withweired$*;_ characters', 4096, 'geom') as mvt FROM (
      SELECT
        ST_AsMVTGeom(
            ST_Transform(ST_CurveToLine("Geom"), 3857),
            ST_TileEnvelope("Z", x, y),
            4096, 64, true) AS geom
      FROM "MixedCase"."MixPoints"
      WHERE "Geom" && ST_Transform(ST_TileEnvelope("Z", x, y), 4326)
  ) as tile WHERE geom IS NOT NULL) src
$$ LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE;

DO $do$ BEGIN
    EXECUTE 'COMMENT ON FUNCTION "public"."""function.withweired$*;_ characters" IS $tj$' || $$
    {
        "description": "a function source with special characters",
        "vector_layers": [
            {
                "id": "public.\"function.withweired$*;_ characters",
                "fields": {
                    "TABLE": "",
                    "Geom": ""
                }
            }
        ]
    }
    $$::json || '$tj$';
END $do$;

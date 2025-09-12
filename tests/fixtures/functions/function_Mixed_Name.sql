DROP FUNCTION IF EXISTS "MixedCase"."function_Mixed_Name";

CREATE OR REPLACE FUNCTION "MixedCase"."function_Mixed_Name"(
    "Z" integer, x integer, y integer
)
RETURNS TABLE ("mVt" bytea, key text) AS $$
  SELECT mvt, md5(mvt) as key FROM (
    SELECT ST_AsMVT(tile, 'MixedCase.function_Mixed_Name', 4096, 'geom') as mvt FROM (
      SELECT
        "Gid",
        ST_AsMVTGeom(
            ST_Transform(ST_CurveToLine("Geom"), 3857),
            ST_TileEnvelope("Z", x, y),
            4096, 64, true) AS geom
      FROM "MixedCase"."MixPoints"
      WHERE "Geom" && ST_Transform(ST_TileEnvelope("Z", x, y), 4326)
  ) as tile WHERE geom IS NOT NULL) src
$$ LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE;

DO $do$ BEGIN
    EXECUTE 'COMMENT ON FUNCTION "MixedCase"."function_Mixed_Name" IS $tj$' || $$
    {
        "description": "a function source with MixedCase name",
        "vector_layers": [
            {
                "id": "MixedCase.function_Mixed_Name",
                "fields": {
                    "TABLE": "",
                    "Geom": ""
                }
            }
        ]
    }
    $$::json || '$tj$';
END $do$;

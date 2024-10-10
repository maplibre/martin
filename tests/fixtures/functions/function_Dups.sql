DROP FUNCTION IF EXISTS "MixedCase".function_dup(int, int, int);
CREATE OR REPLACE FUNCTION "MixedCase".function_dup("Z" int, x int, y int)
RETURNS TABLE("mVt" bytea, key text) AS $$
  SELECT mvt, md5(mvt) as key FROM (
    SELECT ST_AsMVT(tile, '"MixedCase".function_Dup', 4096, 'geom') as mvt FROM (
      SELECT
        ST_AsMVTGeom(
            ST_Transform(ST_CurveToLine("Geom"), 3857),
            ST_TileEnvelope("Z", x, y),
            4096, 64, true) AS geom
      FROM "MixedCase"."MixPoints"
      WHERE "Geom" && ST_Transform(ST_TileEnvelope("Z", x, y), 4326)
  ) as tile WHERE geom IS NOT NULL) src
$$ LANGUAGE SQL IMMUTABLE STRICT PARALLEL SAFE;

DROP FUNCTION IF EXISTS "MixedCase"."function_Dup"(int, int, int);
CREATE OR REPLACE FUNCTION "MixedCase"."function_Dup"("Z" int, x int, y int)
RETURNS TABLE("mVt" bytea, key text) AS $$
  SELECT mvt, md5(mvt) as key FROM (
    SELECT ST_AsMVT(tile, '"MixedCase".function_Dup', 4096, 'geom') as mvt FROM (
      SELECT
        ST_AsMVTGeom(
            ST_Transform(ST_CurveToLine("Geom"), 3857),
            ST_TileEnvelope("Z", x, y),
            4096, 64, true) AS geom
      FROM "MixedCase"."MixPoints"
      WHERE "Geom" && ST_Transform(ST_TileEnvelope("Z", x, y), 4326)
  ) as tile WHERE geom IS NOT NULL) src
$$ LANGUAGE SQL IMMUTABLE STRICT PARALLEL SAFE;

DROP FUNCTION IF EXISTS "MixedCase".function_dup(int, int, int, json);
CREATE OR REPLACE FUNCTION "MixedCase".function_dup(z int, x int, y int, query json)
RETURNS TABLE("mVt" bytea, key text) AS $$
  SELECT mvt, md5(mvt) as key FROM (
    SELECT ST_AsMVT(tile, '"MixedCase".function_Dup', 4096, 'geom') as mvt FROM (
      SELECT
        ST_AsMVTGeom(
            ST_Transform(ST_CurveToLine("Geom"), 3857),
            ST_TileEnvelope(z, x, y),
            4096, 64, true) AS geom
      FROM "MixedCase"."MixPoints"
      WHERE "Geom" && ST_Transform(ST_TileEnvelope(z, x, y), 4326)
  ) as tile WHERE geom IS NOT NULL) src
$$ LANGUAGE SQL IMMUTABLE STRICT PARALLEL SAFE;

DROP FUNCTION IF EXISTS "MixedCase".function_dup(int, int, int, jsonb);
CREATE OR REPLACE FUNCTION "MixedCase".function_dup(z int, x int, y int, query jsonb)
RETURNS TABLE("mVt" bytea, key text) AS $$
  SELECT mvt, md5(mvt) as key FROM (
    SELECT ST_AsMVT(tile, '"MixedCase".function_Dup', 4096, 'geom') as mvt FROM (
      SELECT
        ST_AsMVTGeom(
            ST_Transform(ST_CurveToLine("Geom"), 3857),
            ST_TileEnvelope(z, x, y),
            4096, 64, true) AS geom
      FROM "MixedCase"."MixPoints"
      WHERE "Geom" && ST_Transform(ST_TileEnvelope(z, x, y), 4326)
  ) as tile WHERE geom IS NOT NULL) src
$$ LANGUAGE SQL IMMUTABLE STRICT PARALLEL SAFE;
